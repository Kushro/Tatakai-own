export function startupBanner(origin, listenUrl) {
  const url = new URL(origin).origin;
  const client = url.startsWith('https:') ? 'https://tatakai.me' : 'http://localhost:8090';
  return `\n  TATAKAI / EXTENSION SERVER\n  Sources → Extension runtime → Proxy → Tatakai\n\n  Listening     ${listenUrl}\n  Dashboard     ${url}/\n  Extension API ${url}/api/server/v1\n  Health        ${url}/api/server/v1/health\n  Connect       ${client}/extensionserver/${encodeURIComponent(url)}\n\n  Set the extension API in Tatakai: Settings → Extension Hub → Extension Servers.\n  Press Ctrl+C to stop.\n`;
}
