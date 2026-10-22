/**
 * Shared AniList `Media` selections and the AniList → TatakaiMedia mapper.
 *
 * TatakaiMedia (src/core/content/types.ts) was the normalized shape TatakaiAPI
 * returned; it is almost 1:1 with AniList `Media`. Every direct-AniList content
 * client (anime and manga) selects one of the field sets below and maps the
 * result with `mapAniListMedia`, so UI adapters (toAnimeCard, etc.) keep working.
 *
 * Usage in a query (GraphQL fragments work under aliases too):
 *   `query ($id: Int) { Media(id: $id) { ...mediaCard } } ${MEDIA_CARD_FRAGMENT}`
 */

import type { TatakaiMedia } from '@/core/content/types';

/** Fields for cards, rails, search results. */
export const MEDIA_CARD_FRAGMENT = `
fragment mediaCard on Media {
  id idMal type format status season seasonYear episodes chapters volumes duration
  title { romaji english native } synonyms
  coverImage { extraLarge large medium color } bannerImage
  startDate { year month day } endDate { year month day }
  averageScore meanScore popularity favourites isAdult countryOfOrigin source
  genres
  tags { id name category rank isGeneralSpoiler isMediaSpoiler isAdult }
  studios(isMain: true) { edges { isMain node { id name siteUrl } } }
  nextAiringEpisode { episode airingAt timeUntilAiring }
  description(asHtml: false)
  trailer { id site }
}`;

/** Card fields plus everything the detail pages render. */
export const MEDIA_DETAIL_FRAGMENT = `
fragment mediaDetail on Media {
  ...mediaCard
  studios { edges { isMain node { id name siteUrl } } }
  characters(sort: [ROLE, RELEVANCE, ID], perPage: 25) {
    edges { role node { id name { full } image { large medium } } }
  }
  staff(sort: [RELEVANCE, ID], perPage: 25) {
    edges { role node { id name { full } image { large medium } } }
  }
  relations {
    edges {
      relationType(version: 2)
      node {
        id type format status episodes seasonYear
        title { romaji english }
        coverImage { large medium }
        startDate { year month day }
      }
    }
  }
  externalLinks { id url site type color icon }
  streamingEpisodes { title thumbnail url site }
  rankings { id rank type format year season allTime context }
}
${MEDIA_CARD_FRAGMENT}`;

/** Raw AniList Media as selected by the fragments above (all fields optional). */
export interface AniListMediaRaw {
  id: number;
  idMal?: number | null;
  type?: 'ANIME' | 'MANGA' | null;
  format?: string | null;
  status?: string | null;
  season?: string | null;
  seasonYear?: number | null;
  episodes?: number | null;
  chapters?: number | null;
  volumes?: number | null;
  duration?: number | null;
  title?: { romaji?: string | null; english?: string | null; native?: string | null } | null;
  synonyms?: string[] | null;
  coverImage?: { extraLarge?: string | null; large?: string | null; medium?: string | null; color?: string | null } | null;
  bannerImage?: string | null;
  startDate?: { year?: number | null; month?: number | null; day?: number | null } | null;
  endDate?: { year?: number | null; month?: number | null; day?: number | null } | null;
  averageScore?: number | null;
  meanScore?: number | null;
  popularity?: number | null;
  favourites?: number | null;
  isAdult?: boolean | null;
  countryOfOrigin?: string | null;
  source?: string | null;
  genres?: string[] | null;
  tags?: Array<{ id: number; name: string; category?: string | null; rank?: number | null; isGeneralSpoiler?: boolean | null; isMediaSpoiler?: boolean | null; isAdult?: boolean | null }> | null;
  studios?: { edges?: Array<{ isMain?: boolean | null; node?: { id: number; name: string; siteUrl?: string | null } | null }> | null } | null;
  nextAiringEpisode?: { episode: number; airingAt: number; timeUntilAiring: number } | null;
  description?: string | null;
  trailer?: { id?: string | null; site?: string | null } | null;
  characters?: { edges?: Array<{ role?: string | null; node?: { id: number; name?: { full?: string | null } | null; image?: { large?: string | null; medium?: string | null } | null } | null }> | null } | null;
  staff?: { edges?: Array<{ role?: string | null; node?: { id: number; name?: { full?: string | null } | null; image?: { large?: string | null; medium?: string | null } | null } | null }> | null } | null;
  relations?: { edges?: Array<{ relationType?: string | null; node?: { id: number; type?: string | null; format?: string | null; status?: string | null; episodes?: number | null; seasonYear?: number | null; title?: { romaji?: string | null; english?: string | null } | null; coverImage?: { large?: string | null; medium?: string | null } | null; startDate?: { year?: number | null; month?: number | null; day?: number | null } | null } | null }> | null } | null;
  externalLinks?: Array<{ id?: number | null; url: string; site: string; type?: string | null; color?: string | null; icon?: string | null }> | null;
  streamingEpisodes?: Array<{ title?: string | null; thumbnail?: string | null; url?: string | null; site?: string | null }> | null;
  rankings?: Array<{ id: number; rank: number; type: 'RATED' | 'POPULAR'; format: string; year?: number | null; season?: string | null; allTime?: boolean | null; context: string }> | null;
}

const opt = <T>(v: T | null | undefined): T | undefined => (v == null ? undefined : v);

function fuzzyDate(d: AniListMediaRaw['startDate']): TatakaiMedia['startDate'] {
  if (!d) return undefined;
  const out: Partial<{ year: number; month: number; day: number }> = {};
  if (d.year != null) out.year = d.year;
  if (d.month != null) out.month = d.month;
  if (d.day != null) out.day = d.day;
  return Object.keys(out).length ? out : undefined;
}

/** AniList trailer → watchable URL (YouTube/Dailymotion). */
export function trailerUrlOf(trailer: AniListMediaRaw['trailer']): string | undefined {
  if (!trailer?.id) return undefined;
  if (trailer.site === 'youtube') return `https://www.youtube.com/watch?v=${trailer.id}`;
  if (trailer.site === 'dailymotion') return `https://www.dailymotion.com/video/${trailer.id}`;
  return undefined;
}

/** Map one AniList Media (card or detail selection) to TatakaiMedia. */
export function mapAniListMedia(m: AniListMediaRaw): TatakaiMedia {
  const studioEdges = m.studios?.edges ?? [];
  const seenStudios = new Set<number>();
  const studios = studioEdges
    .filter((e) => e?.node && !seenStudios.has(e.node.id) && seenStudios.add(e.node.id))
    .map((e) => ({ id: e.node!.id, name: e.node!.name, isMain: Boolean(e.isMain), siteUrl: opt(e.node!.siteUrl) }));

  return {
    anilistId: m.id,
    malId: opt(m.idMal),
    titleRomaji: m.title?.romaji || m.title?.english || m.title?.native || '',
    titleEnglish: opt(m.title?.english),
    titleNative: opt(m.title?.native),
    synonyms: m.synonyms ?? [],
    coverImageLarge: opt(m.coverImage?.extraLarge || m.coverImage?.large),
    coverImageMedium: opt(m.coverImage?.medium || m.coverImage?.large),
    bannerImage: opt(m.bannerImage),
    color: opt(m.coverImage?.color),
    format: opt(m.format) as TatakaiMedia['format'],
    status: opt(m.status) as TatakaiMedia['status'],
    season: opt(m.season) as TatakaiMedia['season'],
    seasonYear: opt(m.seasonYear),
    episodes: opt(m.episodes),
    chapters: opt(m.chapters),
    volumes: opt(m.volumes),
    duration: opt(m.duration),
    startDate: fuzzyDate(m.startDate),
    endDate: fuzzyDate(m.endDate),
    description: opt(m.description),
    averageScore: opt(m.averageScore),
    meanScore: opt(m.meanScore),
    popularity: opt(m.popularity),
    favourites: opt(m.favourites),
    genres: m.genres ?? [],
    tags: (m.tags ?? []).map((t) => ({
      id: t.id, name: t.name, category: opt(t.category), rank: opt(t.rank),
      isGeneralSpoiler: opt(t.isGeneralSpoiler), isMediaSpoiler: opt(t.isMediaSpoiler), isAdult: opt(t.isAdult),
    })),
    source: opt(m.source) as TatakaiMedia['source'],
    isAdult: Boolean(m.isAdult),
    countryOfOrigin: opt(m.countryOfOrigin),
    studios,
    characters: m.characters?.edges
      ?.filter((e) => e?.node)
      .map((e) => ({
        id: e.node!.id,
        name: e.node!.name?.full || '',
        imageUrl: opt(e.node!.image?.large || e.node!.image?.medium),
        role: (e.role || 'SUPPORTING') as 'MAIN' | 'SUPPORTING' | 'BACKGROUND',
      })),
    staff: m.staff?.edges
      ?.filter((e) => e?.node)
      .map((e) => ({ id: e.node!.id, name: e.node!.name?.full || '', role: e.role || '', imageUrl: opt(e.node!.image?.large || e.node!.image?.medium) })),
    relations: m.relations?.edges
      ?.filter((e) => e?.node)
      .map((e) => ({
        id: e.node!.id,
        relationType: e.relationType || 'OTHER',
        titleRomaji: e.node!.title?.romaji || e.node!.title?.english || '',
        titleEnglish: opt(e.node!.title?.english),
        coverImage: opt(e.node!.coverImage?.large || e.node!.coverImage?.medium),
        format: opt(e.node!.format) as TatakaiMedia['format'],
        status: opt(e.node!.status) as TatakaiMedia['status'],
        startDate: fuzzyDate(e.node!.startDate),
        seasonYear: opt(e.node!.seasonYear),
        episodes: opt(e.node!.episodes),
      })),
    nextAiringEpisode: opt(m.nextAiringEpisode),
    trailerUrl: trailerUrlOf(m.trailer),
    externalLinks: m.externalLinks?.map((l) => ({ id: opt(l.id), url: l.url, site: l.site, type: opt(l.type), color: opt(l.color), icon: opt(l.icon) })),
    streamingEpisodes: m.streamingEpisodes?.map((s) => ({ title: opt(s.title), thumbnail: opt(s.thumbnail), url: opt(s.url), site: opt(s.site) })),
    rankings: m.rankings?.map((r) => ({
      id: r.id, rank: r.rank, type: r.type, format: r.format as TatakaiMedia['format'],
      year: opt(r.year), season: opt(r.season) as TatakaiMedia['season'], allTime: opt(r.allTime), context: r.context,
    })),
    source_api: 'anilist',
    fetchedAt: Date.now(),
  };
}
