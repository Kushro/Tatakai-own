export type ServerPlatform = 'node' | 'desktop';
export type SourceOperation = 'browse' | 'search' | 'info' | 'episodes' | 'resolve' | 'chapters' | 'pages' | 'novelChapters' | 'novelContent';
export type ApiResponse<T> = { success: true; data: T } | { success: false; error: { code: string; message: string } };
export interface ServerHealth {
  protocolVersion: 1; serverId: string | null; name: string; platform: ServerPlatform; setupRequired: boolean;
  logoUrl: string | null; startedAt: string; uptimeSeconds: number;
  accessPasswordRequired: boolean; adminPasswordRequired: boolean;
  /** true when anonymous access is disabled (desktop host without viewer password): pair the device first. */
  pairingRequired: boolean;
  /** Path of the owner dashboard on the same origin ('/server/'). */
  dashboardPath: string;
  capabilities: Array<{ ecosystem: string; available: boolean; reason: string }>;
  proxy: { hls: boolean; range: boolean; maxResponseBytes: number | null };
  desktop: boolean;
}
/** POST /auth/pairing (owner). url === deepLink; the QR payload is deepLink. */
export interface PairingResponse { code: string; expiresIn: number; url: string; deepLink: string }
/** handle.createPairing(origin) / createDesktopServer().createPairing(origin). */
export interface ProgrammaticPairing { code: string; expiresIn: number; deepLink: string }
/** POST /auth/login, POST /auth/pair, POST /setup. */
export interface ServerToken { token: string; role: 'admin' | 'viewer'; deviceId: string }
export interface SourceDescriptor {
  id: string; extensionId: string; name: string; ecosystem: string; enabled: boolean;
  priority: number; languages: string[]; kind: 'watch' | 'read' | 'novel'; available: boolean; reason: string;
}
export interface InvokeRequest { sourceId: string; op: SourceOperation; args: Record<string, unknown> }
/** Extensions own their provider order. Lower finite ranks are preferred;
 * omitted ranks preserve arrival order after ranked sources. */
export interface SourcePriority { providerPriority?: number }
export interface ServerSettings {
  name: string; languages: string[]; timeoutMs: number; concurrency: number; cacheSeconds: number;
  mediaCacheMb: number; mediaCacheSeconds: number; proxyEnabled: boolean; allowedOrigins: string[];
}
export interface StorageAdapter {
  read(): Promise<{ version: number; data: string } | null>;
  cas(version: number, data: string): Promise<boolean>;
}
export interface ArtifactStore {
  put(key: string, value: string): Promise<unknown>;
  get(key: string): Promise<string>;
  remove(key: string): Promise<unknown>;
}
