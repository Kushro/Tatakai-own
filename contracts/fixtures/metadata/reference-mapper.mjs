// Executable specification of the provider -> core model mapping that BOTH native implementations
// (Kotlin core:metadata, Rust tk-core) must reproduce. `node contracts/fixtures/metadata/gen-expected.mjs` runs it over the
// fixtures and writes expected.json; contracts/test verifies the committed expected.json is current.
//
// Target shape = Media / Episode / FeedPage of docs/redesign/04-implementation-plan.md (3.2). Field names are the Kotlin ones.

const nn = (v) => (v === undefined || v === null || v === '' ? null : v);

/** AniList synopsis is HTML-ish text: <br> -> newline, tags stripped, trailing "(Source: ...)" removed, trimmed. */
export function cleanSynopsis(text) {
  if (nn(text) === null) return null;
  let s = String(text)
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/\s*\((?:Source|Written by)[^)]*\)\s*$/i, '')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
  return s === '' ? null : s;
}

const seasonOfMonth = (m) => (m <= 3 ? 'WINTER' : m <= 6 ? 'SPRING' : m <= 9 ? 'SUMMER' : 'FALL');

export function mapAniListMedia(m) {
  const titles = {};
  if (nn(m.title?.romaji)) titles.romaji = m.title.romaji;
  if (nn(m.title?.english)) titles.english = m.title.english;
  if (nn(m.title?.native)) titles.native = m.title.native;
  return {
    ref: { anilistId: m.id, malId: nn(m.idMal), kitsuId: null },
    kind: m.type === 'MANGA' ? 'MANGA' : 'ANIME',
    title: m.title?.english || m.title?.romaji || m.title?.native || '',
    titles,
    cover: nn(m.coverImage?.extraLarge) ?? nn(m.coverImage?.large),
    banner: nn(m.bannerImage),
    color: nn(m.coverImage?.color),
    synopsis: cleanSynopsis(m.description),
    genres: m.genres ?? [],
    episodes: nn(m.episodes),
    status: nn(m.status),
    season: nn(m.season),
    year: nn(m.seasonYear),
    score: nn(m.averageScore),
    sourceProvider: 'anilist',
  };
}

export function mapAniListPage(doc) {
  const page = doc.data.Page;
  return {
    items: (page.media ?? []).filter((m) => m && m.id).map(mapAniListMedia),
    page: page.pageInfo?.currentPage ?? 1,
    hasNext: Boolean(page.pageInfo?.hasNextPage),
  };
}

/** ani.zip: regular episodes only (numeric keys), ascending. Backticks in titles are apostrophes. */
export function mapAniZipEpisodes(raw) {
  const clean = (v) => (typeof v === 'string' ? v.replace(/`/g, "'").trim() || null : null);
  const out = [];
  for (const [key, e] of Object.entries(raw.episodes ?? {})) {
    if (!/^\d+$/.test(key)) continue;
    const t = e.title ?? {};
    out.push({
      number: Number(key),
      title: clean(t.en) ?? clean(t['x-jat']) ?? clean(t.ja) ?? null,
      image: nn(e.image),
      airDate: nn(e.airDateUtc) ?? nn(e.airDate) ?? nn(e.airdate),
      isFiller: false,
    });
  }
  return out.sort((a, b) => a.number - b.number);
}

export function mapAniZipIds(raw) {
  const m = raw.mappings ?? {};
  return { anilistId: nn(m.anilist_id), malId: nn(m.mal_id), kitsuId: nn(m.kitsu_id) };
}

const KITSU_STATUS = { current: 'RELEASING', finished: 'FINISHED', tba: 'NOT_YET_RELEASED', unreleased: 'NOT_YET_RELEASED', upcoming: 'NOT_YET_RELEASED' };

export function mapKitsuMedia(doc, res) {
  const a = res.attributes;
  // relationship order is preserved
  const included = (type, ids) => ids.map((id) => (doc.included ?? []).find((i) => i.type === type && i.id === id)).filter(Boolean);
  const idsOf = (rel) => (res.relationships?.[rel]?.data ?? []).map((d) => d.id);
  const genres = included('genres', idsOf('genres')).map((g) => g.attributes.name);
  const mappings = included('mappings', idsOf('mappings'));
  const ext = (site) => {
    const row = mappings.find((x) => x.attributes.externalSite === site);
    const n = row ? Number(row.attributes.externalId) : NaN;
    return Number.isFinite(n) ? n : null;
  };
  const start = nn(a.startDate)?.match(/^(\d{4})-(\d{2})/);
  const rating = nn(a.averageRating) === null ? null : Math.round(Number(a.averageRating));
  const titles = {};
  if (nn(a.titles?.en_jp)) titles.romaji = a.titles.en_jp;
  if (nn(a.titles?.en) ?? nn(a.titles?.en_us)) titles.english = nn(a.titles.en) ?? a.titles.en_us;
  if (nn(a.titles?.ja_jp)) titles.native = a.titles.ja_jp;
  return {
    ref: { anilistId: ext('anilist/anime'), malId: ext('myanimelist/anime'), kitsuId: Number(res.id) },
    kind: 'ANIME',
    title: titles.english || a.canonicalTitle || titles.romaji || '',
    titles,
    cover: nn(a.posterImage?.large) ?? nn(a.posterImage?.original),
    banner: nn(a.coverImage?.original) ?? nn(a.coverImage?.large),
    color: null,
    synopsis: cleanSynopsis(a.synopsis ?? a.description),
    genres,
    episodes: nn(a.episodeCount),
    status: KITSU_STATUS[a.status] ?? null,
    season: start ? seasonOfMonth(Number(start[2])) : null,
    year: start ? Number(start[1]) : null,
    score: rating,
    sourceProvider: 'kitsu',
  };
}

export function mapKitsuEpisodes(doc) {
  return doc.data
    .map((r) => {
      const a = r.attributes;
      const number = Number(a.number);
      if (!Number.isFinite(number) || number < 0) return null;
      const th = a.thumbnail ?? {};
      return {
        number,
        title: nn(a.canonicalTitle),
        image: nn(th.original) ?? nn(th.large) ?? nn(th.medium),
        airDate: nn(a.airdate),
        isFiller: false,
      };
    })
    .filter(Boolean)
    .sort((x, y) => x.number - y.number);
}
