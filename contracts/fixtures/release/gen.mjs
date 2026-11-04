// Generator of contracts/fixtures/release/{names,packs}.json (docs/redesign/12 §3.1). Only node:* modules.
// Source of truth: the #[test] functions of apps/desktop-native/crates/tk-core/src/release.rs. Every case names its test in `from`
// (names) / `label` (packs). Cases marked "derived" were traced by hand through the same algorithm (not asserted in Rust yet).
// `expected` is partial: only the keys present are compared. Run: node contracts/fixtures/release/gen.mjs
import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

const n = (from, name, expected) => ({ from, name, expected });

const names = [
  n('subsplease_single_episode', '[SubsPlease] Sousou no Frieren - 12 (1080p) [A1B2C3D4].mkv', {
    group: 'SubsPlease', title: 'Sousou no Frieren', season: null, episodes: [12, 12], resolution: '1080p', source: 'web', codec: null,
    batch: false, dualAudio: false, multiSub: false,
  }),
  n('erai_raws_multi_subs_and_dashed_title',
    '[Erai-raws] Kimetsu no Yaiba - Hashira Geiko-hen - 03 [1080p CR WEB-DL AVC AAC][MultiSub][ENG][POR-BR][SPA-LA][SPA][ARA][FRE][GER][ITA][RUS]', {
      group: 'Erai-raws', title: 'Kimetsu no Yaiba - Hashira Geiko-hen', episodes: [3, 3], resolution: '1080p', source: 'web', codec: 'h264',
      multiSub: true, batch: false,
    }),
  n('erai_raws_multi_subs_and_dashed_title', '[Erai-raws] Show - 01 [720p][ENG][SPA][POR-BR]', { multiSub: true }),
  n('erai_raws_multi_subs_and_dashed_title', '[Erai-raws] Show - 01 [720p][ENG][SPA]', { multiSub: false }),
  n('judas_season_batch', '[Judas] Kimetsu no Yaiba S03 (BD 1080p HEVC 10-bit x265 Dual Audio) [Batch]', {
    group: 'Judas', title: 'Kimetsu no Yaiba', season: 3, episodes: null, source: 'bd', resolution: '1080p', codec: 'hevc', bitDepth: 10,
    dualAudio: true, batch: true,
  }),
  n('asw_hevc_10bit', '[ASW] Dandadan - 05 [1080p HEVC x265 10Bit][AAC]', {
    group: 'ASW', title: 'Dandadan', episodes: [5, 5], codec: 'hevc', bitDepth: 10, source: 'web',
  }),
  n('asw_hevc_10bit', '[Group] Show - 01 [Hi10P 720p]', { bitDepth: 10, resolution: '720p' }),
  n('ember_bdrip_season_pack', '[EMBER] Frieren (2023) (Season 1) [BDRip] [1080p Dual Audio HEVC 10 bits DDP] (Sousou no Frieren)', {
    group: 'EMBER', title: 'Frieren', season: 1, episodes: null, source: 'bd', bitDepth: 10, codec: 'hevc', dualAudio: true, batch: true,
  }),
  n('commie_version_suffix', '[Commie] Show - 01v2 [ABCD1234].mkv', { group: 'Commie', title: 'Show', episodes: [1, 1], source: null }),
  n('scene_and_web_dl_names', 'Sousou no Frieren S01E05 1080p WEB-DL AAC2.0 H.264-VARYG', {
    group: 'VARYG', title: 'Sousou no Frieren', season: 1, episodes: [5, 5], source: 'web', codec: 'h264', resolution: '1080p', batch: false,
  }),
  n('scene_and_web_dl_names', 'Sousou.no.Frieren.S01E05.1080p.NF.WEB.H264-GRP.mkv', {
    group: 'GRP', title: 'Sousou no Frieren', season: 1, episodes: [5, 5], source: 'web',
  }),
  n('scene_and_web_dl_names', 'Show.S02.1080p.BluRay.REMUX.AVC.FLAC.2.0-GRP', { season: 2, episodes: null, source: 'remux', batch: true }),
  n('ranges_and_complete', '[Group] Show 01-12 [BD 1080p]', { title: 'Show', episodes: [1, 12], batch: true }),
  n('ranges_and_complete', '[Group] Show - 01 ~ 12 (Complete) [720p]', { episodes: [1, 12], batch: true, resolution: '720p' }),
  n('ranges_and_complete', '[Group] Show (01-24) [1080p x264]', { title: 'Show', episodes: [1, 24], batch: true }),
  n('ranges_and_complete', '[Group] Show - 01~12 [Batch]', { episodes: [1, 12], batch: true }),
  n('ranges_and_complete', '[Group] Show Complete Series [1080p]', { batch: true, episodes: null }),
  n('remux_av1_and_misc', '[Group] Show [BD REMUX 1080p]', { source: 'remux' }),
  n('remux_av1_and_misc', '[Group] Show - 03 [1080p AV1 OPUS][Multi-Subs]', { codec: 'av1', multiSub: true }),
  n('remux_av1_and_misc', '[Group] One Piece - 1071 [1080p]', { episodes: [1071, 1071] }),
  n('remux_av1_and_misc', 'Kaiju No. 8 - 05 (1080p)', { group: null, title: 'Kaiju No. 8', episodes: [5, 5] }),
  n('remux_av1_and_misc', '[SubsPlease] Re Zero kara Hajimeru Isekai Seikatsu 3rd Season - 05 (720p)', {
    title: 'Re Zero kara Hajimeru Isekai Seikatsu', season: 3, episodes: [5, 5],
  }),
  n('remux_av1_and_misc', '[Group] 86 - Eighty Six - 03 [WEBRip 1080p]', { title: '86 - Eighty Six', episodes: [3, 3], source: 'web' }),
  n('remux_av1_and_misc', '[Group]_Show_-_07_[720p].mkv', { title: 'Show', episodes: [7, 7] }),
  n('remux_av1_and_misc', '[Group] Show EP07 [DVDRip]', { episodes: [7, 7], source: 'dvd' }),
  n('remux_av1_and_misc', '[1080p] Show - 02', { group: null, title: 'Show', resolution: '1080p' }),
  n('never_fails', 'Ünïcödé 名前 - 05', { episodes: [5, 5] }),
  n('never_fails', '', { title: null }),
  // never_fails: these only have to parse without throwing
  ...[' ', '-', '[]', '[', ']', '()', '【】', '[Group]', '....', 's', 'e', '- - -', '01-', '~12', 'v2'].map((name) => n('never_fails', name, {})),
  // derived: file / folder / title names used by the pick_episode_file tests
  n('underscore_names_with_bracketed_episode (derived)', 'Sousou_no_Frieren_[01]_[AniLibria_TV]_[WEBRip_1080p_HEVC].mkv', {
    group: null, title: 'Sousou no Frieren', episodes: [1, 1], resolution: '1080p', source: 'web', codec: 'hevc', batch: false,
  }),
  n('season_pack_skips_extras_and_samples (derived)', '[Judas] Show - S03E02.mkv', {
    group: 'Judas', title: 'Show', season: 3, episodes: [2, 2], source: null, batch: false,
  }),
  n('season_pack_skips_extras_and_samples (derived)', '[Judas] Show S03', { group: 'Judas', title: 'Show', season: 3, episodes: null, batch: true }),
  n('absolute_numbering_pack (derived)', '[Group] Show - 13.mkv', { group: 'Group', title: 'Show', episodes: [13, 13], batch: false }),
  n('absolute_numbering_pack (derived)', 'Show 2nd Season', { title: 'Show', season: 2, episodes: null }),
  n('multi_season_pack_uses_the_titles_season (derived)', 'Season 2', { title: null, season: 2, episodes: null, batch: true }),
  n('multi_season_pack_uses_the_titles_season (derived)', 'Show Season 3', { title: 'Show', season: 3, episodes: null }),
  n('ties_prefer_the_largest_file_and_ranges_match (derived)', '[A] Show - 03 [720p].mkv', {
    group: 'A', title: 'Show', episodes: [3, 3], resolution: '720p', batch: false,
  }),
  n('ties_prefer_the_largest_file_and_ranges_match (derived)', '[A] Show - 04-05 [1080p].mkv', {
    group: 'A', title: 'Show', episodes: [4, 5], resolution: '1080p', batch: true,
  }),
  n('single_video_file_is_picked (derived)', 'Show/[Group] Show - 01 (1080p).mkv', { group: 'Group', title: 'Show', episodes: [1, 1], resolution: '1080p' }),
  n('resolution_of (derived)', '[Group] Show - 01 [1920x1080]', { episodes: [1, 1], resolution: '1080p' }),
  n('resolution_of (derived)', '[Group] Show - 05 [4K]', { episodes: [5, 5], resolution: '2160p' }),
];

const f = (index, path, sizeBytes) => ({ index, path, sizeBytes });
const MB = 1024 * 1024;

const anilibria = Array.from({ length: 28 }, (_, i) => {
  const ep = String(i + 1).padStart(2, '0');
  return f(i, `Sousou no Frieren - AniLibria.TV [WEBRip 1080p HEVC]/Sousou_no_Frieren_[${ep}]_[AniLibria_TV]_[WEBRip_1080p_HEVC].mkv`, 300 * MB);
});
const frierenTitles = ['Frieren: Beyond Journey’s End', 'Sousou no Frieren', '葬送のフリーレン'];
const single = [f(0, 'Show/info.nfo', 10), f(1, 'Show/[Group] Show - 01 (1080p).mkv', 1024 * MB)];
const judas = [
  f(0, '[Judas] Show S03/[Judas] Show - S03E01.mkv', 900),
  f(1, '[Judas] Show S03/[Judas] Show - S03E02.mkv', 900),
  f(2, '[Judas] Show S03/Extras/[Judas] Show - NCOP 02.mkv', 50),
  f(3, '[Judas] Show S03/[Judas] Show - S03E02 sample.mkv', 5),
  f(4, '[Judas] Show S03/[Judas] Show - S03E02.ass', 1),
  f(5, '[Judas] Show S03/[Judas] Show - NCED2.mkv', 40),
];
const absolute = Array.from({ length: 12 }, (_, i) => f(i, `Show S2/[Group] Show - ${i + 13}.mkv`, 1000));
const multi = [
  f(0, 'Show/Season 1/[Group] Show - 01 [1080p].mkv', 1000),
  f(1, 'Show/Season 2/[Group] Show - 01 [1080p].mkv', 1000),
  f(2, 'Show/Season 1/[Group] Show - 02 [1080p].mkv', 1000),
];
const ties = [f(7, '[A] Show - 03 [720p].mkv', 300), f(8, '[A] Show - 03 [1080p].mkv', 900), f(9, '[A] Show - 04-05 [1080p].mkv', 1800)];
const ova = [f(0, 'Show/[G] Show - OVA 01.mkv', 500), f(1, 'Show/[G] Show - 01.mkv', 500), f(2, 'Show/[G] Show - 02.mkv', 500)];

const p = (label, files, episode, titles, expected) => ({ label, files, episode, titles, expected });
const packs = [
  p('underscore_names_with_bracketed_episode: ep 1', anilibria, 1, frierenTitles, 0),
  p('underscore_names_with_bracketed_episode: ep 12', anilibria, 12, frierenTitles, 11),
  p('underscore_names_with_bracketed_episode: ep 28', anilibria, 28, frierenTitles, 27),
  p('single_video_file_is_picked', single, 1, ['Show'], 1),
  p('season_pack_skips_extras_and_samples: titles say season 3', judas, 2, ['Show Season 3'], 1),
  p('season_pack_skips_extras_and_samples: ep 2', judas, 2, ['Show'], 1),
  p('season_pack_skips_extras_and_samples: ep 1', judas, 1, ['Show'], 0),
  p('season_pack_skips_extras_and_samples: missing episode', judas, 9, ['Show'], null),
  p('absolute_numbering_pack: ep 1 of season 2', absolute, 1, ['Show 2nd Season'], 0),
  p('absolute_numbering_pack: ep 12 of season 2', absolute, 12, ['Show 2nd Season'], 11),
  p('absolute_numbering_pack: absolute number asked directly', absolute, 14, ['Show'], 1),
  p('absolute_numbering_pack: beyond the pack', absolute, 25, ['Show'], null),
  p('multi_season_pack_uses_the_titles_season: Season 2 + II', multi, 1, ['Show Season 2', 'Show II'], 1),
  p('multi_season_pack_uses_the_titles_season: II', multi, 1, ['Show II'], 1),
  p('multi_season_pack_uses_the_titles_season: no season', multi, 1, ['Show'], 0),
  p('multi_season_pack_uses_the_titles_season: season 3 not in the pack', multi, 2, ['Show Season 3'], 2),
  p('ties_prefer_the_largest_file_and_ranges_match: largest of two', ties, 3, ['Show'], 8),
  p('ties_prefer_the_largest_file_and_ranges_match: range file', ties, 5, ['Show'], 9),
  p('ties_prefer_the_largest_file_and_ranges_match: empty torrent', [], 1, ['Show'], null),
  p('ties_prefer_the_largest_file_and_ranges_match: single file of another episode', ties.slice(0, 1), 4, ['Show'], null),
  p('ova_files_are_kept_when_the_titles_ask_for_them: OVA skipped', ova, 1, ['Show'], 1),
  p('ova_files_are_kept_when_the_titles_ask_for_them: OVA asked (derived: tie 110/110, same size, lowest index)', ova, 1, ['Show OVA'], 0),
];

const write = (file, comment, cases) =>
  writeFileSync(join(here, file), `${JSON.stringify({ $comment: comment, cases }, null, 2)}\n`, 'utf8');

write(
  'names.json',
  'parseRelease oracle (tk-core release.rs tests; Kotlin :core:release ReleaseFixturesTest). `expected` is a partial ReleaseTags in desktop serde form: only the keys present are compared; null asserts absence; {} = must not throw. `from` = source test. Generated by gen.mjs.',
  names,
);
write(
  'packs.json',
  'pickEpisodeFile oracle (tk-core release.rs tests; Kotlin :core:release ReleaseFixturesTest). `expected` = the returned file index or null. `label` = source test. Generated by gen.mjs.',
  packs,
);
console.log(`names.json: ${names.length} cases, packs.json: ${packs.length} cases`);
