import { useEffect, useRef } from 'react';
import { supabase } from '@/integrations/supabase/client';
import { useAuth } from '@/contexts/AuthContext';
import { analytics } from '@/core/analytics/AnalyticsService';
import { getVisitSessionId } from '@/lib/analyticsIdentity';

// Check if we're online
function isOnline(): boolean {
  return typeof navigator !== 'undefined' ? navigator.onLine : true;
}

// Generate or get session ID from localStorage
const getSessionId = getVisitSessionId;

// ---------------------------------------------------------------------------
// Traffic attribution — shared by the tracker (stored per visit) and the admin
// dashboard (fallback for rows written before the attribution columns existed).
// ---------------------------------------------------------------------------

export type TrafficChannel =
  | 'Direct'
  | 'Organic Search'
  | 'Social'
  | 'Referral'
  | 'Internal'
  | 'Paid';

const SEARCH_HOSTS = ['google', 'bing', 'yahoo', 'duckduckgo', 'baidu', 'yandex', 'ecosia', 'brave', 'ask.com', 'aol'];
const SOCIAL_HOSTS = [
  'facebook', 'twitter', 'x.com', 't.co', 'instagram', 'reddit', 'tiktok',
  'youtube', 'youtu.be', 'discord', 'telegram', 't.me', 'tumblr', 'pinterest',
  'linkedin', 'twitch', 'whatsapp',
];
const SITE_HOSTS = ['tatakai.me', 'api.tatakai.me'];

/** Extract the hostname of a referrer URL (lowercased), or null. */
export function referrerHostOf(referrer?: string | null): string | null {
  if (!referrer) return null;
  try {
    return new URL(referrer).hostname.toLowerCase();
  } catch {
    return null;
  }
}

/** Classify a visit into a traffic channel from referrer + utm_medium. */
export function classifyChannel(referrer?: string | null, utmMedium?: string | null): TrafficChannel {
  const medium = (utmMedium || '').toLowerCase();
  if (['cpc', 'ppc', 'paid', 'cpm', 'cpa', 'display', 'paid-social', 'paidsocial'].includes(medium)) {
    return 'Paid';
  }
  const host = referrerHostOf(referrer);
  if (!host) return 'Direct';
  if (SITE_HOSTS.some((h) => host === h || host.endsWith(`.${h}`))) return 'Internal';
  if (SEARCH_HOSTS.some((h) => host.includes(h))) return 'Organic Search';
  if (SOCIAL_HOSTS.some((h) => host.includes(h))) return 'Social';
  return 'Referral';
}

// Start watch session - returns session ID
export async function startWatchSession(
  animeId: string,
  episodeId: string,
  userId?: string,
  metadata?: { animeName?: string; animePoster?: string; genres?: string[] }
): Promise<string | null> {
  // Skip when offline
  if (!isOnline()) return null;
  
  try {
    const sessionId = getSessionId();

    const { data, error } = await supabase
      .from('watch_sessions')
      .insert({
        user_id: userId || null,
        session_id: sessionId,
        anime_id: animeId,
        episode_id: episodeId,
        anime_name: metadata?.animeName || 'Unknown',
        anime_poster: metadata?.animePoster,
        genres: metadata?.genres,
      })
      .select('id')
      .single();

    if (error) throw error;
    return data?.id || null;
  } catch (error) {
    if (import.meta.env.DEV) {
      console.warn('Failed to start watch session:', error);
    }
    return null;
  }
}

// Update watch session duration
export async function updateWatchSession(
  watchSessionId: string,
  durationSeconds: number
): Promise<void> {
  // Skip when offline
  if (!isOnline()) return;
  
  try {
    await supabase
      .from('watch_sessions')
      .update({
        watch_duration_seconds: durationSeconds,
        end_time: new Date().toISOString(),
      })
      .eq('id', watchSessionId);
  } catch (error) {
    if (import.meta.env.DEV) {
      console.warn('Failed to update watch session:', error);
    }
  }
}

// Hook to track page visits on navigation
export function usePageTracking(enabled: boolean = true) {
  const lastPath = useRef<string>('');

  useEffect(() => {
    if (!enabled || typeof window === 'undefined') return;

    const trackCurrentPage = () => {
      const currentPath = window.location.pathname;

      // Only track if path changed
      if (currentPath !== lastPath.current) {
        lastPath.current = currentPath;
        analytics.trackPageView(currentPath);
      }
    };

    const locationChangeEvent = 'tatakai:location-change';

    const originalPushState = window.history.pushState;
    const originalReplaceState = window.history.replaceState;

    window.history.pushState = function (...args) {
      const result = originalPushState.apply(this, args as [data: any, unused: string, url?: string | URL | null]);
      window.dispatchEvent(new Event(locationChangeEvent));
      return result;
    };

    window.history.replaceState = function (...args) {
      const result = originalReplaceState.apply(this, args as [data: any, unused: string, url?: string | URL | null]);
      window.dispatchEvent(new Event(locationChangeEvent));
      return result;
    };

    let initialTrackTimeout: ReturnType<typeof setTimeout> | null = null;
    let initialIdleHandle: number | null = null;

    if ('requestIdleCallback' in window) {
      initialIdleHandle = (window as any).requestIdleCallback(trackCurrentPage, { timeout: 1200 });
    } else {
      initialTrackTimeout = setTimeout(trackCurrentPage, 450);
    }

    // Listen for popstate (browser back/forward)
    window.addEventListener('popstate', trackCurrentPage);
    window.addEventListener(locationChangeEvent, trackCurrentPage);

    return () => {
      if (initialTrackTimeout !== null) clearTimeout(initialTrackTimeout);
      if (initialIdleHandle !== null && 'cancelIdleCallback' in window) {
        (window as any).cancelIdleCallback(initialIdleHandle);
      }

      window.removeEventListener('popstate', trackCurrentPage);
      window.removeEventListener(locationChangeEvent, trackCurrentPage);
      window.history.pushState = originalPushState;
      window.history.replaceState = originalReplaceState;
    };
  }, [enabled]);
}

// Hook to track watch time
export function useWatchTracking(
  animeId: string,
  episodeId: string,
  metadata?: { animeName?: string; animePoster?: string; genres?: string[] }
) {
  const { user } = useAuth();
  const watchSessionIdRef = useRef<string | null>(null);
  const startTimeRef = useRef<number>(Date.now());
  const updateIntervalRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    // Start session
    const initSession = async () => {
      watchSessionIdRef.current = await startWatchSession(
        animeId,
        episodeId,
        user?.id,
        metadata
      );
      startTimeRef.current = Date.now();
    };

    initSession();

    // Update every 30 seconds
    updateIntervalRef.current = setInterval(() => {
      if (watchSessionIdRef.current) {
        const durationSeconds = Math.floor((Date.now() - startTimeRef.current) / 1000);
        updateWatchSession(watchSessionIdRef.current, durationSeconds);
      }
    }, 30000);

    // Cleanup on unmount
    return () => {
      if (updateIntervalRef.current) {
        clearInterval(updateIntervalRef.current);
      }
      if (watchSessionIdRef.current) {
        const durationSeconds = Math.floor((Date.now() - startTimeRef.current) / 1000);
        updateWatchSession(watchSessionIdRef.current, durationSeconds);
      }
    };
  }, [animeId, episodeId, user?.id, metadata]);

  return watchSessionIdRef;
}
