import ExtensionServerLinkPage from './ExtensionServerLinkPage';

/**
 * `/connect-server?server=…&code=…` (older pairing links). Same confirmation
 * flow as `/extensionserver/connect`: ExtensionServerLinkPage parses both, and
 * the index.html head script has already moved the code out of the address bar.
 */
export default function ConnectServerPage() {
  return <ExtensionServerLinkPage />;
}
