export const BUILTIN_FONT_FAMILY = 'Noto Sans CJK SC';
export const BUILTIN_FONT_BYTES = 19484784 + 20050760;
const files = ['NotoSansCJK-Regular.ttc', 'NotoSansCJK-Bold.ttc'];
let cached;

/** Keep originals in the host and transfer a fresh copy to each Worker load. */
export async function loadBuiltinFonts(signal) {
  if (!cached) {
    const request = Promise.all(files.map(async name => {
      const response = await fetch(new URL(`./fonts/${name}`, import.meta.url), { signal });
      if (!response.ok) throw new Error(`内置字体加载失败（${response.status}）：${name}。请重试或检查字体资源。`);
      const bytes = await response.arrayBuffer();
      if (!bytes.byteLength || bytes.byteLength > 64 * 1024 * 1024) throw new Error(`内置字体大小无效：${name}`);
      return { name, bytes };
    }));
    cached = request;
    request.catch(() => { if (cached === request) cached = undefined; });
  }
  const fonts = await cached;
  if (signal?.aborted) throw signal.reason;
  return fonts.map(font => ({ name: font.name, bytes: font.bytes.slice(0) }));
}
