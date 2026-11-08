import { getQuickJS } from 'quickjs-emscripten';
import { evaluateSora, type ScriptVMHost } from '../shared/tatakai/src/core/extensions/sora/vm';
import { evaluateMangayomi } from '../shared/tatakai/src/core/extensions/mangayomi/vm';
import { createMangayomiDom } from '../shared/tatakai/src/core/extensions/mangayomi/dom';
import { limitedText, fail } from './security.mjs';
import { transform } from 'sucrase';

// Only serialized HTTP and the existing DOM broker cross the interpreter boundary.
// No extension code is evaluated in the host JavaScript engine.
export function createInterpreter(loadEngine = getQuickJS) {
  return async function interpret({ code, ecosystem, manifest, method, args = {}, preferences = {}, network, signal, probe = false }: any) {
    if (ecosystem === 'seanime' && manifest.language === 'typescript') code = transform(code, { transforms: ['typescript'], disableESTransforms: true }).code;
    const engine = await loadEngine();
    const http = async (request: any) => {
      if (ecosystem === 'tatakai' && Array.isArray(manifest.permissions)) {
        const domains = manifest.permissions.filter((value: string) => value.startsWith('network:domain:')).map((value: string) => value.slice(15).toLowerCase());
        const host = new URL(request.url).hostname.toLowerCase();
        if (!domains.some((domain: string) => domain === '*' || domain === host || domain.startsWith('*.') && host.endsWith(domain.slice(1)))) fail(403, 'network_permission_denied');
      }
      const headers = request.headers || {};
      const response = await network(request.url, { method: request.method || 'GET', headers,
        body: ['GET', 'HEAD'].includes(request.method || 'GET') ? undefined : request.body, signal });
      return { status: response.response.status, headers: Object.fromEntries(response.response.headers), body: await limitedText(response.response), url: response.url };
    };
    const callArgs = ecosystem === 'sora' ? [args.url || args.id || args.query || args.title || '', args.page || 1] : [args];
    if (ecosystem === 'sora') return evaluateSora(engine, code, probe ? 'probe' : method, callArgs, http, signal);
    if (ecosystem === 'mangayomi') {
      const values = probe ? [] : method === 'search' ? [args.query || args.title || '', args.page || 1, []] : [args.url || args.id || args.chapterId || args.page || 1];
      const result = await evaluateMangayomi(engine, code, probe ? 'probe' : method, values, { source: manifest.source || manifest, preferences }, http, createMangayomiDom(), signal) as any;
      return result?.value;
    }
    if (!method && !probe) fail(422, 'unsupported_operation');
    const host: ScriptVMHost = {
      methods: [method || 'probe'], filename: `${ecosystem}-extension.js`,
      setup: `
        const module={exports:{}}, exports=module.exports;
        const self=globalThis;
        const __tatakai_fetch__=fetch;
        const __preferences=${JSON.stringify(preferences)};
        const $getUserPreference=key=>__preferences[key];
        const $toString=value=>String(value), $toBytes=value=>Array.from(String(value),c=>c.charCodeAt(0));
        const $store={get:key=>__preferences[key],set(){throw Error('Persistent provider store is unavailable')}};
        class AbstractSource {}
        const require=name=>{if(name==='@tatakai/sdk')return {AbstractSource};throw Error('Unsupported module: '+name)};
      `,
      expression: (op, values) => `(async()=>{
        const exported=module.exports.default||module.exports;
        const provider=${ecosystem === 'seanime' ? "typeof Provider==='function'?new Provider():" : ''}typeof exported==='function'?new exported():exported;
        if(${probe}) {
          if(!provider || !Object.getOwnPropertyNames(Object.getPrototypeOf(provider)||{}).concat(Object.keys(provider)).some(key=>typeof provider[key]==='function'))throw Error('No source methods exported');
          return JSON.stringify({valid:true});
        }
        let method=${JSON.stringify(op)};
        const args=${JSON.stringify(values)};
        if(${JSON.stringify(ecosystem)}==='tatakai' && method==='single' && typeof provider.single!=='function')method=typeof provider.sourcesAll==='function'?'sourcesAll':typeof provider.getSources==='function'?'getSources':'single';
        if(${JSON.stringify(ecosystem)}==='seanime') {
          const options=args[0];
          if(method==='getInfo' && typeof provider.getInfo!=='function')return JSON.stringify({id:options.id,title:options.title||options.id,entries:await provider[${JSON.stringify(manifest.type === 'manga-provider' ? 'findChapters' : 'findEpisodes')}](options.id)});
          if(method==='findChapters'||method==='findEpisodes')return JSON.stringify(await provider[method](options.id||options.titleId));
          if(method==='findChapterPages')return JSON.stringify(await provider[method](options.chapterId||options.id));
          if(method==='resolve') {
            const episodes=await provider.findEpisodes(options.titleId||options.id);
            const episode=episodes.find(item=>String(item.id||item.url)===String(options.episodeId)||Number(item.number)===Number(options.episode||1));
            if(!episode)return '[]';
            const settings=await provider.getSettings();const sources=[];
            for(const server of settings.episodeServers||[]) {
              const result=await provider.findEpisodeServer(episode,server);
              for(const video of result.videoSources||[])sources.push({url:video.url,quality:video.quality,headers:result.headers||{},sourceType:video.type==='m3u8'?'hls':'mp4',subtitles:video.subtitles||[]});
            }
            return JSON.stringify(sources);
          }
        }
        if(typeof provider[method]!=='function')throw Error('Unsupported extension method: '+method);
        return JSON.stringify(await provider[method](...args));
      })()`,
    };
    return evaluateSora(engine, code, probe ? 'probe' : method, [{ ...args, preferences, titles: args.titles || [args.title].filter(Boolean), resolution: args.resolution || '1080p' }], http, signal, host);
  };
}
export const interpreter = createInterpreter();
