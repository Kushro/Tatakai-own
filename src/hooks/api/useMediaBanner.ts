import { useQuery } from "@tanstack/react-query";
import { anilistGql } from "@/lib/anilist/client";

/**
 * Lazy AniList banner art for the quick-peek sheet.
 *
 * Cards only carry posters; the wide banner behind the sheet's header comes
 * from here when the row has a numeric AniList id. Purely progressive
 * enhancement — any failure (rate limit, missing banner) falls back to the
 * blurred-poster header, so the query never blocks the sheet.
 */
export function useMediaBanner(
  kind: "anime" | "manga" | "novel" | undefined,
  anilistId: number | null | undefined,
  enabled: boolean,
) {
  return useQuery({
    queryKey: ["media-banner", kind, anilistId],
    queryFn: async (): Promise<string | null> => {
      if (!anilistId || !kind) return null;
      let data: any;
      try {
        data = await anilistGql<any>(
          `query ($id: Int, $type: MediaType) { Media(id: $id, type: $type) { bannerImage } }`,
          { id: anilistId, type: kind === "anime" ? "ANIME" : "MANGA" },
        );
      } catch {
        return null;
      }
      const banner = data?.Media?.bannerImage;
      return typeof banner === "string" && banner ? banner : null;
    },
    enabled: enabled && !!kind && !!anilistId && anilistId > 0,
    staleTime: 60 * 60 * 1000,
    gcTime: 24 * 60 * 60 * 1000,
    retry: 1,
  });
}
