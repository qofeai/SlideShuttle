"""Send one benchmark generation turn to an existing interactive Claude in Rex.

No claude -p, no status polling, no writes to the model's work directory.
The caller explicitly selects the session, block and the conversation log.
"""
import argparse
import hashlib
import json
import selectors
import subprocess
import time
from pathlib import Path


def records(value):
    if isinstance(value, dict):
        if "state" in value and "updated_at" in value:
            yield value
        for child in value.values():
            yield from records(child)
    elif isinstance(value, list):
        for child in value:
            yield from records(child)


def main():
    p = argparse.ArgumentParser()
    for name in ("prompt", "out", "session", "block", "conversation"):
        p.add_argument(f"--{name}", required=True)
    p.add_argument("--timeout", type=int, default=900)
    a = p.parse_args()
    out = Path(a.out)
    out.mkdir(exist_ok=True, parents=True)
    conversation = Path(a.conversation)
    offset = conversation.stat().st_size
    prompt = Path(a.prompt).read_text()
    start = time.monotonic()
    events = subprocess.Popen(["rex", "events", "-s", a.session, "--json"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    selector = selectors.DefaultSelector()
    selector.register(events.stdout, selectors.EVENT_READ)
    outcome = "timeout"
    working = False
    try:
        status = subprocess.check_output(["rex", "block", "call", "-s", a.session, "-b", a.block, "program_status"], text=True)
        (out / "initial-status.json").write_text(status)
        subprocess.run(["rex", "send", "-s", a.session, "-b", a.block], input=prompt, text=True, check=True)
        subprocess.run(["rex", "send-key", "-s", a.session, "-b", a.block, "enter"], check=True)
        print("Benchmark prompt sent through interactive Rex session", flush=True)
        with (out / "rex-events.jsonl").open("w") as log:
            while time.monotonic() - start < a.timeout:
                if not selector.select(timeout=30):
                    if events.poll() is not None:
                        outcome = "event_stream_ended"
                        break
                    continue
                line = events.stdout.readline()
                if not line:
                    outcome = "event_stream_ended"
                    break
                if a.block not in line:
                    continue
                log.write(line)
                log.flush()
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                for r in records(event):
                    state = r.get("state")
                    if state == "working":
                        working = True
                    elif state in ("blocked", "error"):
                        outcome = state
                    elif working and state in ("done", "idle"):
                        outcome = "complete"
                if outcome != "timeout":
                    break
    finally:
        events.terminate()
        try:
            events.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            events.kill()
            events.communicate()
    with conversation.open("rb") as f:
        f.seek(offset)
        raw = f.read()
    (out / "conversation-turn.jsonl").write_bytes(raw)
    turns = [json.loads(line) for line in raw.splitlines() if line.strip()]
    messages = [r["message"] for r in turns if r.get("type") == "assistant"]
    text = "\n".join(c.get("text", "") for m in messages for c in m.get("content", []) if c.get("type") == "text")
    (out / "response.md").write_text(text)
    metadata = {"outcome": outcome, "elapsedSeconds": time.monotonic() - start, "promptSha256": hashlib.sha256(prompt.encode()).hexdigest(), "models": sorted({m.get("model", "unknown") for m in messages}), "usage": [m.get("usage", {}) for m in messages], "method": "existing interactive Rex Claude session; prior architecture discussion context retained", "workingEventSeen": working}
    (out / "generation.json").write_text(json.dumps(metadata, indent=2))
    print(json.dumps(metadata), flush=True)
    if outcome != "complete" or not text:
        raise SystemExit(2)


if __name__ == "__main__":
    main()
