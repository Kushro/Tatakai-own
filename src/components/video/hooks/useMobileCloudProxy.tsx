import type { RefObject } from 'react';

/**
 * Inert in this fork. Upstream's opt-in cloud video proxy (hoko.tatakai.me) is
 * not part of this build: video always loads on the device. The hook keeps its
 * signature so MobileVideoPlayer stays diff-compatible with upstream.
 */
const NO_RESUME = { current: 0 };

export function useMobileCloudProxy(_options: {
  source?: { url: string; originalUrl?: string; sourceType?: string; headers?: Record<string, string> };
  headers?: Record<string, string | undefined>;
  enabled: boolean;
  videoRef: RefObject<HTMLVideoElement>;
}) {
  return {
    cloudProxyUrl: null as string | null,
    offerCloudProxy: (_onDecline?: () => void): boolean => false,
    cloudProxyDialog: null,
    cloudResumePosition: NO_RESUME,
  };
}
