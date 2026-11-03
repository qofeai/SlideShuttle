//! Slide-show behaviour: page types, transitions, animations and speaker notes. None of it
//! changes the static page. PPTX gets native timing; `render --steps` draws the page after
//! each click.
use crate::model::*;
use crate::parser::{attrs, required};
use roxmltree::Node as X;
use std::collections::{BTreeMap, BTreeSet};

/// `fade`, `push left`, `wipe up slow`, `none`.
pub(crate) fn transition(value: &str) -> Result<Option<Transition>> {
    let usage = || {
        invalid(format!(
            "transition {value:?}: write `<fade|push|wipe|cover|split|zoom|cut> [up|down|left|right] [fast|medium|slow]` or `none`"
        ))
    };
    let mut words = value.split_whitespace();
    let kind = match words.next() {
        None => return Err(usage()),
        Some("none") if words.next().is_none() => return Ok(None),
        Some(k) => keyword::<TransitionKind>(k, "transition").map_err(|_| usage())?,
    };
    let mut t = Transition {
        kind,
        direction: None,
        speed: Speed::Medium,
    };
    let mut speed = None;
    for w in words {
        if let Ok(d) = keyword::<Direction>(w, "") {
            if !matches!(
                kind,
                TransitionKind::Push | TransitionKind::Wipe | TransitionKind::Cover
            ) || t.direction.replace(d).is_some()
            {
                return Err(invalid(format!(
                    "transition {value:?}: only push, wipe and cover take one direction"
                )));
            }
        } else if speed
            .replace(keyword(w, "").map_err(|_| usage())?)
            .is_some()
        {
            return Err(usage());
        }
    }
    t.speed = speed.unwrap_or(Speed::Medium);
    Ok(Some(t))
}

/// `500ms` or `0.5s`, up to a minute.
fn millis(value: &str, what: &str) -> Result<u32> {
    let v = value.trim();
    let ms = if let Some(n) = v.strip_suffix("ms") {
        n.trim().parse::<f64>().ok()
    } else {
        v.strip_suffix('s')
            .and_then(|n| n.trim().parse::<f64>().ok())
            .map(|s| s * 1000.0)
    };
    ms.filter(|m| m.is_finite() && (0.0..=60_000.0).contains(m))
        .map(|m| m.round() as u32)
        .ok_or_else(|| {
            invalid(format!(
                "Animate {what} is a time like 500ms or 0.5s, up to 60s"
            ))
        })
}

fn directed(e: Effect) -> bool {
    use Effect::*;
    matches!(
        e,
        FlyIn | FlyOut | WipeIn | WipeOut | PeekIn | FloatIn | FloatOut
    )
}
fn entrance(e: Effect) -> bool {
    use Effect::*;
    matches!(
        e,
        Appear | FadeIn | FlyIn | ZoomIn | WipeIn | FloatIn | PeekIn | RiseIn
    )
}
fn exit(e: Effect) -> bool {
    use Effect::*;
    matches!(
        e,
        Disappear | FadeOut | FlyOut | ZoomOut | WipeOut | FloatOut
    )
}
fn default_duration(e: Effect) -> u32 {
    use Effect::*;
    match e {
        Appear | Disappear => 0,
        Pulse => 600,
        Teeter | RiseIn => 1000,
        GrowShrink | Spin | FillColor | Transparency | ColorPulse | MotionPath => 2000,
        _ => 500,
    }
}

/// A motion path: one subpath from `M 0 0`, with lines and curves.
fn motion_path(d: &str) -> Result<Vec<crate::render::Command>> {
    use crate::render::Command;
    let commands = crate::icon::path_commands(d)?;
    if !matches!(commands.first(), Some(Command::M(x, y)) if *x == 0.0 && *y == 0.0)
        || commands.iter().skip(1).any(|c| matches!(c, Command::M(..)))
        || commands.len() > 200
    {
        return Err(invalid(
            "motion-path is one subpath starting at M 0 0, at most 200 segments",
        ));
    }
    Ok(commands)
}

pub(crate) fn animation(n: X<'_, '_>, ids: &BTreeSet<String>) -> Result<Animation> {
    attrs(
        n,
        &[
            "target",
            "effect",
            "trigger",
            "direction",
            "duration",
            "delay",
            "easing",
            "repeat",
            "color",
            "amount",
            "path",
        ],
    )?;
    if n.has_children() {
        return Err(invalid("<Animate> has no content"));
    }
    let target = required(n, "target")?;
    if !ids.contains(target) {
        return Err(invalid(format!(
            "Animate target {target} is not an element on this slide"
        )));
    }
    let effect: Effect = keyword(required(n, "effect")?, "Animate effect")?;
    let get = |k: &str| n.attribute(k);
    let rule = |ok: bool, message: &str| {
        if ok {
            Ok(())
        } else {
            Err(invalid(format!("Animate {target} {message}")))
        }
    };
    let direction: Option<Direction> = get("direction")
        .map(|v| keyword(v, "Animate direction"))
        .transpose()?;
    rule(
        direction.is_none() || directed(effect),
        "takes a direction only with fly, wipe, peek and float effects",
    )?;
    rule(
        !matches!(effect, Effect::FloatIn | Effect::FloatOut)
            || matches!(direction, None | Some(Direction::Up | Direction::Down)),
        "floats only up or down",
    )?;
    let duration = get("duration").map(|v| millis(v, "duration")).transpose()?;
    rule(
        duration.is_none() || !matches!(effect, Effect::Appear | Effect::Disappear),
        "appear and disappear are instant and take no duration",
    )?;
    rule(duration != Some(0), "duration must be positive")?;
    let repeat = match get("repeat") {
        Some(v) => v
            .parse::<u32>()
            .ok()
            .filter(|r| (1..=100).contains(r))
            .ok_or_else(|| invalid("Animate repeat is a whole number from 1 to 100"))?,
        None => 1,
    };
    let color = get("color").map(Color::parse).transpose()?;
    let colored = matches!(effect, Effect::FillColor | Effect::ColorPulse);
    rule(
        color.is_some() == colored,
        "needs color exactly when the effect is fill-color or color-pulse",
    )?;
    let amount = get("amount")
        .map(|v| {
            v.parse::<f32>()
                .ok()
                .filter(|a| (0.0..=1.0).contains(a))
                .ok_or_else(|| invalid("Animate amount is an opacity from 0 to 1"))
        })
        .transpose()?;
    rule(
        amount.is_some() == (effect == Effect::Transparency),
        "needs amount exactly when the effect is transparency",
    )?;
    let path = get("path").map(str::to_owned);
    rule(
        path.is_some() == (effect == Effect::MotionPath),
        "needs path exactly when the effect is motion-path",
    )?;
    if let Some(p) = &path {
        motion_path(p)?;
    }
    Ok(Animation {
        target: target.into(),
        effect,
        trigger: get("trigger")
            .map(|v| keyword(v, "Animate trigger"))
            .transpose()?
            .unwrap_or(Trigger::Click),
        direction,
        duration,
        delay: get("delay")
            .map(|v| millis(v, "delay"))
            .transpose()?
            .unwrap_or(0),
        easing: get("easing")
            .map(|v| keyword(v, "Animate easing"))
            .transpose()?
            .unwrap_or(Easing::Linear),
        repeat,
        color,
        amount,
        path,
    })
}

/// One paragraph per non-empty line.
pub(crate) fn notes(n: X<'_, '_>) -> Result<Vec<String>> {
    attrs(n, &[])?;
    if n.children().any(|c| c.is_element()) {
        return Err(invalid(
            "<Notes> holds plain text; start each paragraph on a new line",
        ));
    }
    let text = n.text().unwrap_or("");
    if text.len() > 64 * 1024 {
        return Err(invalid("speaker notes exceed 64 KiB"));
    }
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

// ---- Sequencing ----

/// Click groups in order: whether the group starts on its own when the slide opens, and its
/// subgroups (animations starting together), each with its start time.
struct Group {
    auto: bool,
    subs: Vec<(u32, Vec<usize>)>,
}
fn length(a: &Animation) -> u32 {
    a.delay + a.duration.unwrap_or(default_duration(a.effect)) * a.repeat
}
fn groups(animations: &[Animation]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, a) in animations.iter().enumerate() {
        match (a.trigger, out.last_mut()) {
            (Trigger::WithPrevious, Some(g)) => g.subs.last_mut().unwrap().1.push(i),
            (Trigger::AfterPrevious, Some(g)) => {
                let (start, last) = g.subs.last().unwrap();
                let end = start
                    + last
                        .iter()
                        .map(|j| length(&animations[*j]))
                        .max()
                        .unwrap_or(0);
                g.subs.push((end, vec![i]));
            }
            (trigger, _) => out.push(Group {
                auto: trigger != Trigger::Click,
                subs: vec![(0, vec![i])],
            }),
        }
    }
    out
}

/// How an element looks after some animations have played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub hidden: bool,
    pub opacity: f32,
    pub fill: Option<Color>,
    pub scale: f32,
    pub offset: (f32, f32),
}
impl Default for Look {
    fn default() -> Self {
        Self {
            hidden: false,
            opacity: 1.0,
            fill: None,
            scale: 1.0,
            offset: (0.0, 0.0),
        }
    }
}

/// The page before the first click, then after each click: the looks that differ from
/// the static page. Elements whose first animation is an entrance start hidden.
pub fn steps(show: &Show) -> Vec<BTreeMap<String, Look>> {
    let mut looks = BTreeMap::<String, Look>::new();
    for a in &show.animations {
        if !looks.contains_key(&a.target) {
            looks.insert(
                a.target.clone(),
                Look {
                    hidden: entrance(a.effect),
                    ..Look::default()
                },
            );
        }
    }
    let mut out = Vec::new();
    let play = |looks: &mut BTreeMap<String, Look>, g: &Group| {
        for i in g.subs.iter().flat_map(|(_, s)| s) {
            let a = &show.animations[*i];
            let look = looks.get_mut(&a.target).unwrap();
            match a.effect {
                e if entrance(e) => look.hidden = false,
                e if exit(e) => look.hidden = true,
                Effect::Transparency => look.opacity = a.amount.unwrap_or(1.0),
                Effect::FillColor => look.fill = a.color,
                Effect::GrowShrink => look.scale *= 1.5,
                Effect::MotionPath => {
                    if let Some(
                        crate::render::Command::M(x, y)
                        | crate::render::Command::L(x, y)
                        | crate::render::Command::C(.., x, y),
                    ) = a
                        .path
                        .as_deref()
                        .and_then(|p| motion_path(p).ok())
                        .and_then(|c| {
                            c.into_iter()
                                .rev()
                                .find(|c| !matches!(c, crate::render::Command::Z))
                        })
                    {
                        look.offset.0 += x;
                        look.offset.1 += y;
                    }
                }
                _ => {}
            }
        }
    };
    let groups = groups(&show.animations);
    let mut rest = groups.iter().peekable();
    if let Some(g) = rest.next_if(|g| g.auto) {
        play(&mut looks, g);
    }
    out.push(looks.clone());
    for g in rest {
        play(&mut looks, g);
        out.push(looks.clone());
    }
    for step in &mut out {
        step.retain(|_, l| *l != Look::default());
    }
    out
}

// ---- PPTX ----

struct Ids(u32);
impl Ids {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}
/// Behaviour writer for one target shape.
struct Target<'a> {
    ids: &'a mut Ids,
    spid: u32,
}
impl Target<'_> {
    fn behavior(&mut self, dur: u32, extra: &str, delay: Option<u32>, names: &[&str]) -> String {
        let id = self.ids.next();
        let cond = delay
            .map(|d| format!("<p:stCondLst><p:cond delay=\"{d}\"/></p:stCondLst>"))
            .unwrap_or_default();
        let additive = if names.iter().any(|n| n.starts_with("ppt_")) {
            " additive=\"base\""
        } else {
            ""
        };
        let names = if names.is_empty() {
            String::new()
        } else {
            format!(
                "<p:attrNameLst>{}</p:attrNameLst>",
                names
                    .iter()
                    .map(|a| format!("<p:attrName>{a}</p:attrName>"))
                    .collect::<String>()
            )
        };
        format!(
            "<p:cBhvr{additive}><p:cTn id=\"{id}\" dur=\"{}\" fill=\"hold\"{extra}>{cond}</p:cTn><p:tgtEl><p:spTgt spid=\"{}\"/></p:tgtEl>{names}</p:cBhvr>",
            dur.max(1),
            self.spid
        )
    }
    fn set(&mut self, at: u32, dur: u32, name: &str, value: &str) -> String {
        format!(
            "<p:set>{}<p:to><p:strVal val=\"{value}\"/></p:to></p:set>",
            self.behavior(dur, "", Some(at), &[name])
        )
    }
    fn show(&mut self, visible: bool, at: u32) -> String {
        self.set(
            at,
            1,
            "style.visibility",
            if visible { "visible" } else { "hidden" },
        )
    }
    fn filter(&mut self, dur: u32, out: bool, filter: &str) -> String {
        format!(
            "<p:animEffect transition=\"{}\" filter=\"{filter}\">{}</p:animEffect>",
            if out { "out" } else { "in" },
            self.behavior(dur, "", None, &[])
        )
    }
    fn anim(&mut self, dur: u32, name: &str, from: &str, to: &str) -> String {
        format!(
            "<p:anim calcmode=\"lin\" valueType=\"num\">{}<p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"{from}\"/></p:val></p:tav><p:tav tm=\"100000\"><p:val><p:strVal val=\"{to}\"/></p:val></p:tav></p:tavLst></p:anim>",
            self.behavior(dur, "", None, &[name])
        )
    }
    fn scale(&mut self, dur: u32, by: u32, reverse: bool) -> String {
        format!(
            "<p:animScale>{}<p:by x=\"{by}\" y=\"{by}\"/></p:animScale>",
            self.behavior(dur, if reverse { " autoRev=\"1\"" } else { "" }, None, &[])
        )
    }
    fn rotate(&mut self, dur: u32, at: u32, degrees: i32) -> String {
        format!(
            "<p:animRot by=\"{}\">{}</p:animRot>",
            degrees * 60000,
            self.behavior(dur, "", Some(at), &["r"])
        )
    }
    fn color(&mut self, dur: u32, c: Color, reverse: bool) -> String {
        format!(
            "<p:animClr clrSpc=\"rgb\" dir=\"cw\">{}<p:to><a:srgbClr val=\"{}\"/></p:to></p:animClr>",
            self.behavior(
                dur,
                if reverse { " autoRev=\"1\"" } else { "" },
                None,
                &["fillcolor"]
            ),
            c.hex()
        )
    }
}

/// Off-page position (`ppt_x`, `ppt_y`) an element flies from or to, moving in `d`.
fn off_page(d: Direction, entering: bool) -> (&'static str, &'static str) {
    match (d, entering) {
        (Direction::Up, true) | (Direction::Down, false) => ("#ppt_x", "1+#ppt_h/2"),
        (Direction::Down, true) | (Direction::Up, false) => ("#ppt_x", "0-#ppt_h/2"),
        (Direction::Left, true) | (Direction::Right, false) => ("1+#ppt_w/2", "#ppt_y"),
        (Direction::Right, true) | (Direction::Left, false) => ("0-#ppt_w/2", "#ppt_y"),
    }
}
fn wipe(d: Direction) -> &'static str {
    match d {
        Direction::Up => "wipe(up)",
        Direction::Down => "wipe(down)",
        Direction::Left => "wipe(left)",
        Direction::Right => "wipe(right)",
    }
}
fn peek(d: Direction) -> &'static str {
    match d {
        Direction::Up => "slide(fromBottom)",
        Direction::Down => "slide(fromTop)",
        Direction::Left => "slide(fromRight)",
        Direction::Right => "slide(fromLeft)",
    }
}

/// PowerPoint's preset class, id and subtype for an effect.
fn preset(a: &Animation) -> (&'static str, u32, u32) {
    use Effect::*;
    let d = a.direction.unwrap_or(Direction::Up);
    // Entrances name the side they come from; exits the side they leave by.
    let from = match d {
        Direction::Up => 4,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 8,
    };
    let to = match d {
        Direction::Up => 1,
        Direction::Down => 4,
        Direction::Left => 8,
        Direction::Right => 2,
    };
    // Float presets: 42 ascends and 47 descends on entry; exits use them the other way round.
    let down = d == Direction::Down;
    let float = if down == exit(a.effect) { 42 } else { 47 };
    match a.effect {
        Appear => ("entr", 1, 0),
        FadeIn => ("entr", 10, 0),
        FlyIn => ("entr", 2, from),
        ZoomIn => ("entr", 53, 16),
        WipeIn => ("entr", 22, from),
        FloatIn => ("entr", float, 0),
        PeekIn => ("entr", 12, from),
        RiseIn => ("entr", 37, 0),
        Pulse => ("emph", 26, 0),
        GrowShrink => ("emph", 6, 0),
        Spin => ("emph", 8, 0),
        Teeter => ("emph", 32, 0),
        FillColor => ("emph", 1, 0),
        Transparency => ("emph", 9, 0),
        ColorPulse => ("emph", 27, 0),
        Disappear => ("exit", 1, 0),
        FadeOut => ("exit", 10, 0),
        FlyOut => ("exit", 2, to),
        ZoomOut => ("exit", 53, 16),
        // A wipe names where it starts, leaving or entering.
        WipeOut => ("exit", 22, from),
        FloatOut => ("exit", float, 0),
        MotionPath => ("path", 0, 0),
    }
}

fn behaviors(t: &mut Target<'_>, a: &Animation, page: (f32, f32)) -> String {
    use Effect::*;
    let d = a.duration.unwrap_or(default_duration(a.effect));
    let dir = a.direction.unwrap_or(Direction::Up);
    let end = d.saturating_sub(1);
    let up = dir == Direction::Up;
    match a.effect {
        Appear => t.show(true, 0),
        FadeIn => t.show(true, 0) + &t.filter(d, false, "fade"),
        FlyIn => {
            let (x, y) = off_page(dir, true);
            t.show(true, 0) + &t.anim(d, "ppt_x", x, "#ppt_x") + &t.anim(d, "ppt_y", y, "#ppt_y")
        }
        ZoomIn => {
            t.show(true, 0)
                + &t.anim(d, "ppt_w", "0", "#ppt_w")
                + &t.anim(d, "ppt_h", "0", "#ppt_h")
                + &t.filter(d, false, "fade")
        }
        WipeIn => t.show(true, 0) + &t.filter(d, false, wipe(dir)),
        FloatIn => {
            let from = if up { "#ppt_y+.1" } else { "#ppt_y-.1" };
            t.show(true, 0) + &t.filter(d, false, "fade") + &t.anim(d, "ppt_y", from, "#ppt_y")
        }
        PeekIn => t.show(true, 0) + &t.filter(d, false, peek(dir)),
        RiseIn => t.show(true, 0) + &t.anim(d, "ppt_y", "#ppt_y+1", "#ppt_y"),
        Pulse => t.scale(d / 2, 110000, true),
        GrowShrink => t.scale(d, 150000, false),
        Spin => t.rotate(d, 0, 360),
        Teeter => {
            let step = d / 5;
            [4, -8, 8, -8, 4]
                .iter()
                .enumerate()
                .map(|(i, deg)| t.rotate(step, i as u32 * step, *deg))
                .collect()
        }
        FillColor => {
            t.set(0, d, "fill.type", "solid")
                + &t.set(0, d, "fill.on", "true")
                + &t.color(d, a.color.unwrap_or_default(), false)
        }
        Transparency => {
            let amount = a.amount.unwrap_or(1.0);
            t.set(0, d, "style.opacity", &amount.to_string())
                + &format!(
                    "<p:animEffect filter=\"image\" prLst=\"opacity: {amount}\">{}</p:animEffect>",
                    t.behavior(d, "", None, &[])
                )
        }
        ColorPulse => t.color(d / 2, a.color.unwrap_or_default(), true),
        Disappear => t.show(false, 0),
        FadeOut => t.filter(d, true, "fade") + &t.show(false, end),
        FlyOut => {
            let (x, y) = off_page(dir, false);
            t.anim(d, "ppt_x", "#ppt_x", x) + &t.anim(d, "ppt_y", "#ppt_y", y) + &t.show(false, end)
        }
        ZoomOut => {
            t.anim(d, "ppt_w", "#ppt_w", "0")
                + &t.anim(d, "ppt_h", "#ppt_h", "0")
                + &t.filter(d, true, "fade")
                + &t.show(false, end)
        }
        WipeOut => t.filter(d, true, wipe(dir)) + &t.show(false, end),
        FloatOut => {
            let to = if up { "#ppt_y-.1" } else { "#ppt_y+.1" };
            t.filter(d, true, "fade") + &t.anim(d, "ppt_y", "#ppt_y", to) + &t.show(false, end)
        }
        MotionPath => {
            let path = motion_path(a.path.as_deref().unwrap_or("M 0 0"))
                .unwrap_or_default()
                .iter()
                .map(|c| {
                    use crate::render::Command::*;
                    let p = |x: &f32, y: &f32| format!("{} {}", x / page.0, y / page.1);
                    match c {
                        M(x, y) => format!("M {}", p(x, y)),
                        L(x, y) => format!("L {}", p(x, y)),
                        C(a, b, c, d, e, f) => format!("C {} {} {}", p(a, b), p(c, d), p(e, f)),
                        Z => "Z".into(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "<p:animMotion origin=\"layout\" path=\"{path} E\" pathEditMode=\"relative\" ptsTypes=\"\">{}</p:animMotion>",
                t.behavior(d, "", None, &["ppt_x", "ppt_y"])
            )
        }
    }
}

/// `<p:timing>` for a slide; `shapes` maps element ids to their shapes: id and whether it
/// is a `p:sp` (which PowerPoint lists in the build list).
pub(crate) fn timing(
    show: &Show,
    shapes: &BTreeMap<String, Vec<(u32, bool)>>,
    page: (f32, f32),
) -> Result<String> {
    if show.animations.is_empty() {
        return Ok(String::new());
    }
    let mut ids = Ids(2);
    let mut builds = Vec::new();
    let mut seen = BTreeMap::<u32, u32>::new();
    let mut sequence = String::new();
    for g in groups(&show.animations) {
        let outer = ids.next();
        let start = if g.auto {
            "<p:cond delay=\"indefinite\"/><p:cond evt=\"onBegin\" delay=\"0\"><p:tn val=\"2\"/></p:cond>"
        } else {
            "<p:cond delay=\"indefinite\"/>"
        };
        let mut subs = String::new();
        for (k, (at, members)) in g.subs.iter().enumerate() {
            let sub = ids.next();
            let mut effects = String::new();
            for (j, i) in members.iter().enumerate() {
                let a = &show.animations[*i];
                let parts = shapes.get(&a.target).ok_or_else(|| {
                    invalid(format!("animation target {} draws nothing", a.target))
                })?;
                let (class, preset_id, subtype) = preset(a);
                let mut timing = String::new();
                match a.easing {
                    Easing::Linear => {}
                    Easing::EaseIn => timing.push_str(" accel=\"50000\""),
                    Easing::EaseOut => timing.push_str(" decel=\"50000\""),
                    Easing::EaseInOut => timing.push_str(" accel=\"50000\" decel=\"50000\""),
                }
                if a.repeat > 1 {
                    timing.push_str(&format!(" repeatCount=\"{}\"", a.repeat * 1000));
                }
                // A Text's painted box is its own shape; both play the same effect together.
                for (m, &(spid, sp)) in parts.iter().enumerate() {
                    let grp = seen.entry(spid).or_insert(0);
                    let group_id = *grp;
                    *grp += 1;
                    if sp {
                        builds.push(format!(
                            "<p:bldP spid=\"{spid}\" grpId=\"{group_id}\" animBg=\"1\"/>"
                        ));
                    }
                    let node = match (k, j, m, a.trigger) {
                        (0, 0, 0, Trigger::Click) => "clickEffect",
                        (_, _, 0, Trigger::AfterPrevious) => "afterEffect",
                        _ => "withEffect",
                    };
                    let id = ids.next();
                    let body = behaviors(
                        &mut Target {
                            ids: &mut ids,
                            spid,
                        },
                        a,
                        page,
                    );
                    effects.push_str(&format!(
                        "<p:par><p:cTn id=\"{id}\" presetID=\"{preset_id}\" presetClass=\"{class}\" presetSubtype=\"{subtype}\"{timing} fill=\"hold\" grpId=\"{group_id}\" nodeType=\"{node}\"><p:stCondLst><p:cond delay=\"{}\"/></p:stCondLst><p:childTnLst>{body}</p:childTnLst></p:cTn></p:par>",
                        a.delay
                    ));
                }
            }
            subs.push_str(&format!(
                "<p:par><p:cTn id=\"{sub}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"{at}\"/></p:stCondLst><p:childTnLst>{effects}</p:childTnLst></p:cTn></p:par>"
            ));
        }
        sequence.push_str(&format!(
            "<p:par><p:cTn id=\"{outer}\" fill=\"hold\"><p:stCondLst>{start}</p:stCondLst><p:childTnLst>{subs}</p:childTnLst></p:cTn></p:par>"
        ));
    }
    let builds = if builds.is_empty() {
        String::new()
    } else {
        format!("<p:bldLst>{}</p:bldLst>", builds.concat())
    };
    Ok(format!(
        "<p:timing><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst><p:seq concurrent=\"1\" nextAc=\"seek\"><p:cTn id=\"2\" dur=\"indefinite\" nodeType=\"mainSeq\"><p:childTnLst>{sequence}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt=\"onPrev\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt=\"onNext\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst>{builds}</p:timing>"
    ))
}

fn side(d: Direction) -> &'static str {
    match d {
        Direction::Up => "u",
        Direction::Down => "d",
        Direction::Left => "l",
        Direction::Right => "r",
    }
}
pub(crate) fn transition_xml(t: Option<&Transition>) -> String {
    let Some(t) = t else {
        return String::new();
    };
    let dir = t
        .direction
        .map(|d| format!(" dir=\"{}\"", side(d)))
        .unwrap_or_default();
    let effect = match t.kind {
        TransitionKind::Fade => "<p:fade/>".into(),
        TransitionKind::Push => format!("<p:push{dir}/>"),
        TransitionKind::Wipe => format!("<p:wipe{dir}/>"),
        TransitionKind::Cover => format!("<p:cover{dir}/>"),
        TransitionKind::Split => "<p:split orient=\"vert\" dir=\"out\"/>".into(),
        TransitionKind::Zoom => "<p:zoom/>".into(),
        TransitionKind::Cut => "<p:cut/>".into(),
    };
    let speed = match t.speed {
        Speed::Fast => "fast",
        Speed::Medium => "med",
        Speed::Slow => "slow",
    };
    format!("<p:transition spd=\"{speed}\">{effect}</p:transition>")
}

/// The notes page body: one paragraph per note line.
pub(crate) fn notes_xml(notes: &[String], p: &str, a: &str, r: &str) -> String {
    let paragraphs = notes
        .iter()
        .map(|n| {
            format!(
                "<a:p><a:r><a:rPr lang=\"zh-CN\"/><a:t>{}</a:t></a:r></a:p>",
                xml(n)
            )
        })
        .collect::<String>();
    format!(
        "<p:notes xmlns:p=\"{p}\" xmlns:a=\"{a}\" xmlns:r=\"{r}\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
    )
}

// ---- Import ----

fn attr<'a>(n: X<'a, '_>, k: &str) -> Option<&'a str> {
    n.attribute(k)
}
fn named<'a, 'i>(n: X<'a, 'i>, name: &str) -> impl Iterator<Item = X<'a, 'i>> {
    let name = name.to_owned();
    n.descendants().filter(move |c| c.tag_name().name() == name)
}

/// A slide's transition as source; `None` with a problem when it has no equivalent.
pub(crate) fn import_transition(t: X<'_, '_>, problems: &mut BTreeSet<String>) -> Option<String> {
    let effect = t.children().find(|c| c.is_element())?;
    let kind = match effect.tag_name().name() {
        "fade" if attr(effect, "thruBlk") != Some("1") => "fade",
        "push" => "push",
        "wipe" => "wipe",
        "cover" => "cover",
        "split" => "split",
        "zoom" => "zoom",
        "cut" => "cut",
        other => {
            problems.insert(format!("{other} transition is not reconstructed"));
            return None;
        }
    };
    let mut words = vec![kind.to_owned()];
    if ["push", "wipe", "cover"].contains(&kind)
        && let Some(d) = attr(effect, "dir").filter(|d| *d != "l")
    {
        words.push(
            match d {
                "u" => "up",
                "d" => "down",
                "r" => "right",
                _ => "left",
            }
            .into(),
        );
    }
    match attr(t, "spd") {
        Some("fast") => words.push("fast".into()),
        Some("slow") => words.push("slow".into()),
        _ => {}
    }
    Some(words.join(" "))
}

/// The main click sequence as `<Animate>` elements. `targets` maps shape ids to the elements
/// the shape became, which then animate together; effects with no equivalent are problems.
pub(crate) fn import_timing(
    timing: X<'_, '_>,
    targets: &BTreeMap<String, Vec<String>>,
    page: (f32, f32),
    problems: &mut BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if named(timing, "cTn").any(|c| attr(c, "nodeType") == Some("interactiveSeq")) {
        problems.insert("trigger-on-shape animations are not reconstructed".into());
    }
    let effects = named(timing, "cTn").filter(|c| attr(*c, "presetClass").is_some());
    for c in effects {
        let class = attr(c, "presetClass").unwrap_or("");
        let id = attr(c, "presetID")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let subtype = attr(c, "presetSubtype")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        use Effect::*;
        let found = [
            Appear,
            FadeIn,
            FlyIn,
            ZoomIn,
            WipeIn,
            FloatIn,
            PeekIn,
            RiseIn,
            Pulse,
            GrowShrink,
            Spin,
            Teeter,
            FillColor,
            Transparency,
            ColorPulse,
            Disappear,
            FadeOut,
            FlyOut,
            ZoomOut,
            WipeOut,
            FloatOut,
            MotionPath,
        ]
        .into_iter()
        .flat_map(|e| {
            [
                None,
                Some(Direction::Up),
                Some(Direction::Down),
                Some(Direction::Left),
                Some(Direction::Right),
            ]
            .into_iter()
            .filter(move |d| d.is_none() || directed(e))
            .map(move |d| (e, d))
        })
        .find(|(e, d)| {
            let probe = Animation {
                target: String::new(),
                effect: *e,
                trigger: Trigger::Click,
                direction: *d,
                duration: None,
                delay: 0,
                easing: Easing::Linear,
                repeat: 1,
                color: None,
                amount: None,
                path: None,
            };
            let (pc, pi, ps) = preset(&probe);
            pc == class && pi == id && (ps == subtype || !directed(*e))
        });
        let Some((effect, direction)) = found else {
            problems.insert(format!(
                "{class} animation preset {id} is not reconstructed"
            ));
            continue;
        };
        let shape = named(c, "spTgt").next();
        let Some(target) = shape
            .filter(|t| t.children().all(|c| !c.is_element()))
            .and_then(|t| attr(t, "spid"))
            .and_then(|s| targets.get(s))
        else {
            problems.insert(
                "animation of a paragraph, or of an object that is not reconstructed, is dropped"
                    .into(),
            );
            continue;
        };
        let trigger = match attr(c, "nodeType") {
            Some("withEffect") => " trigger=\"with-previous\"",
            Some("afterEffect") => " trigger=\"after-previous\"",
            _ => "",
        };
        let mut a = format!(
            " effect=\"{}\"",
            serde_json::to_value(effect)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        );
        let direction = direction.filter(|d| *d != Direction::Up);
        if let Some(d) = direction {
            a.push_str(&format!(
                " direction=\"{}\"",
                match d {
                    Direction::Up => "up",
                    Direction::Down => "down",
                    Direction::Left => "left",
                    Direction::Right => "right",
                }
            ));
        }
        let durs = c
            .descendants()
            .filter(|n| n.tag_name().name() == "cTn" && *n != c)
            .filter_map(|n| attr(n, "dur")?.parse::<u32>().ok())
            .filter(|d| *d > 1)
            .collect::<Vec<_>>();
        let duration = match effect {
            Pulse | ColorPulse => durs.first().map(|d| d * 2),
            Teeter => Some(durs.iter().sum()),
            _ => durs.iter().max().copied(),
        };
        if let Some(d) = duration
            .filter(|d| *d != default_duration(effect) && !matches!(effect, Appear | Disappear))
        {
            a.push_str(&format!(" duration=\"{d}ms\""));
        }
        if let Some(d) = c
            .children()
            .find(|n| n.tag_name().name() == "stCondLst")
            .and_then(|s| named(s, "cond").next())
            .and_then(|n| attr(n, "delay")?.parse::<u32>().ok())
            .filter(|d| *d > 0)
        {
            a.push_str(&format!(" delay=\"{d}ms\""));
        }
        match (attr(c, "accel").is_some(), attr(c, "decel").is_some()) {
            (true, true) => a.push_str(" easing=\"ease-in-out\""),
            (true, false) => a.push_str(" easing=\"ease-in\""),
            (false, true) => a.push_str(" easing=\"ease-out\""),
            _ => {}
        }
        if let Some(r) = attr(c, "repeatCount")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|r| *r > 1000)
        {
            a.push_str(&format!(" repeat=\"{}\"", (r / 1000).clamp(1, 100)));
        }
        if matches!(effect, FillColor | ColorPulse) {
            let color = named(c, "srgbClr")
                .next()
                .and_then(|n| attr(n, "val"))
                .unwrap_or("000000");
            a.push_str(&format!(" color=\"#{color}\""));
        }
        if effect == Transparency {
            let amount = named(c, "strVal")
                .filter_map(|v| attr(v, "val")?.parse::<f32>().ok())
                .next()
                .unwrap_or(1.0);
            a.push_str(&format!(" amount=\"{amount}\""));
        }
        if effect == MotionPath {
            let raw = named(c, "animMotion")
                .next()
                .and_then(|m| attr(m, "path"))
                .unwrap_or("M 0 0");
            let mut axis = 0;
            let path = raw
                .split_whitespace()
                .filter(|w| *w != "E")
                .map(|w| match w.parse::<f32>() {
                    Ok(v) => {
                        let scale = if axis % 2 == 0 { page.0 } else { page.1 };
                        axis += 1;
                        format!("{}", (v * scale * 100.0).round() / 100.0)
                    }
                    Err(_) => {
                        axis = 0;
                        w.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            a.push_str(&format!(" path=\"{}\"", xml(&path)));
        }
        for (k, id) in target.iter().enumerate() {
            let trigger = if k == 0 {
                trigger
            } else {
                " trigger=\"with-previous\""
            };
            out.push(format!("<Animate target=\"{}\"{trigger}{a}/>", xml(id)));
        }
    }
    out
}

/// Notes page text, one line per paragraph.
pub(crate) fn import_notes(notes: X<'_, '_>) -> Vec<String> {
    named(notes, "sp")
        .filter(|sp| named(*sp, "ph").any(|ph| attr(ph, "type") == Some("body")))
        .flat_map(|sp| named(sp, "p").collect::<Vec<_>>())
        .map(|p| named(p, "t").filter_map(|t| t.text()).collect::<String>())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_and_times() {
        let t = transition("push up fast").unwrap().unwrap();
        assert_eq!(
            (t.kind, t.direction, t.speed),
            (TransitionKind::Push, Some(Direction::Up), Speed::Fast)
        );
        assert!(transition("none").unwrap().is_none());
        assert!(transition("fade left").is_err());
        assert!(transition("spin").is_err());
        assert_eq!(millis("0.5s", "").unwrap(), 500);
        assert_eq!(millis("250ms", "").unwrap(), 250);
        assert!(millis("500", "").is_err());
    }
}
