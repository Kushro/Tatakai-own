import { useState } from "react";
import type { useCustomCatalog } from "./useCustomCatalog";
import type { ExtensionLibraryItem } from "@/core/extensions/compatibility/library";

/**
 * Disabled in this fork.
 *
 * Upstream syncs extension progress to AniList/MAL by POSTing the Supabase
 * session JWT to TatakaiAPI (`/sync/extension-progress`). This fork never sends
 * its JWT to a third-party backend and keeps AniList/MAL tokens in an owner-only
 * table, so the hook is a no-op that preserves the signature. Local progress is
 * still saved by the player/reader; a client-side tracker sync (same code path
 * as `src/lib/externalIntegrations.ts`) can replace this later.
 */
export function useCustomTracker(
  _catalog: ReturnType<typeof useCustomCatalog>,
  _entries: { id: string; number?: number }[],
  _row?: ExtensionLibraryItem
) {
  const [, setRevision] = useState(0);
  return { error: "", retry: () => setRevision((value) => value + 1) };
}
