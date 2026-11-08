export type ServerPlatform = 'node' | 'desktop' | 'vercel' | 'workers';
export type SourceOperation = 'browse' | 'search' | 'info' | 'episodes' | 'resolve' | 'chapters' | 'pages' | 'novelChapters' | 'novelContent';
export type ApiResponse<T> = { success: true; data: T } | { success: false; error: { code: string; message: string } };
export interface ServerHealth {
  protocolVersion: 1; serverId: string | null; name: string; platform: ServerPlatform; setupRequired: boolean;
  accessPasswordRequired: boolean; adminPasswordRequired: boolean;
  capabilities: Array<{ ecosystem: string; available: boolean; reason: string }>;
  proxy: { hls: boolean; range: boolean; maxResponseBytes: number | null };
  desktop: boolean;
}
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
