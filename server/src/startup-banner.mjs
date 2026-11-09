// The banner never points at a hosted client: phones pair through the dashboard QR (tatakai:// deep link).
export function startupBanner(origin, listenUrl, pairing) {
  const url = new URL(origin).origin;
  const link = pairing?.deepLink ? `\n  Pair a device ${pairing.deepLink}\n                (one use, expires in ${Math.round((pairing.expiresIn || 300) / 60)} min)\n` : '';
  return `\n  TATAKAI / EXTENSION SERVER\n  Sources → Extension runtime → Proxy → Tatakai\n\n  Listening     ${listenUrl}\n  Dashboard     ${url}/server/\n  Extension API ${url}/api/server/v1\n  Health        ${url}/api/server/v1/health\n${link}\n  Pair a phone: open the dashboard, create a pairing QR code and scan it in Tatakai,\n  or add ${url} in Tatakai: Settings → Extension Hub → Extension Servers.\n  Press Ctrl+C to stop.\n`;
}
