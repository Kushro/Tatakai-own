import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { ArrowDown, ArrowUp, Database, RotateCcw } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { queryClient } from '@/lib/queryClient';
import { contentGraph } from '@/core/content/content-graph';
import { db } from '@/core/db';
import {
  SettingRow,
  SettingsBadge,
  SettingsSection,
} from '@/components/settings/SettingsPrimitives';
import {
  getMetadataSources,
  getProviderHealth,
  listMetadataProviders,
  resetMetadataSources,
  resetProviderHealth,
  setMetadataSources,
  subscribeMetadataSources,
  type MetadataProvider,
  type MetadataSourceSetting,
} from '@/lib/metadata';

const HEALTH_REFRESH_MS = 15_000;

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(uiI18n.language, { hour: '2-digit', minute: '2-digit' });
}

/**
 * Catalog data depends on the source order, so every cache layer is stale after
 * a change: content-graph memory/localStorage entries, the Dexie media rows and
 * react-query.
 */
function applyChange(): void {
  resetProviderHealth();
  contentGraph.invalidateAll();
  void db.cachedMedia.clear().catch(() => undefined).then(() => queryClient.invalidateQueries());
}

/**
 * Settings → Metadata sources: the user-ordered list of catalog providers.
 * The first enabled entry is the primary source; the rest are fallbacks.
 */
export function MetadataSourcesPanel() {
  useUiTranslation();
  const [sources, setSources] = useState<MetadataSourceSetting[]>(() => getMetadataSources());
  // Bumped on a timer / after changes so the health column re-reads the router.
  const [, setHealthTick] = useState(0);

  useEffect(() => subscribeMetadataSources(setSources), []);
  useEffect(() => {
    const timer = setInterval(() => setHealthTick((n) => n + 1), HEALTH_REFRESH_MS);
    return () => clearInterval(timer);
  }, []);

  const providers = useMemo(() => {
    const map = new Map<string, MetadataProvider>();
    for (const p of listMetadataProviders()) map.set(p.id, p);
    return map;
  }, []);

  const commit = useCallback((next: MetadataSourceSetting[]) => {
    setMetadataSources(next);
    applyChange();
    setHealthTick((n) => n + 1);
  }, []);

  const toggle = (id: string, enabled: boolean) =>
    commit(sources.map((s) => (s.id === id ? { ...s, enabled } : s)));

  const move = (index: number, delta: -1 | 1) => {
    const target = index + delta;
    if (target < 0 || target >= sources.length) return;
    const next = [...sources];
    [next[index], next[target]] = [next[target], next[index]];
    commit(next);
  };

  const reset = () => {
    resetMetadataSources();
    applyChange();
    setHealthTick((n) => n + 1);
    toast.success(uiI18n.t('metadataSources.resetDone'));
  };

  const primaryId = sources.find((s) => s.enabled && providers.get(s.id)?.isConfigured())?.id
    ?? sources.find((s) => s.enabled)?.id;
  const health = getProviderHealth();
  const now = Date.now();
  const noneEnabled = !sources.some((s) => s.enabled);

  return (
    <SettingsSection
      icon={Database}
      eyebrow="Catalog"
      title={uiI18n.t('metadataSources.title')}
      description={uiI18n.t('metadataSources.description')}
      action={
        <Button type="button" variant="outline" size="sm" onClick={reset} className="gap-2">
          <RotateCcw className="h-3.5 w-3.5" />
          {uiI18n.t('metadataSources.reset')}
        </Button>
      }
    >
      {noneEnabled && (
        <p role="alert" className="mb-2 text-sm text-amber">
          {uiI18n.t('metadataSources.allDisabled')}
        </p>
      )}
      <ol className="space-y-1" aria-label={uiI18n.t('metadataSources.title')}>
        {sources.map((source, index) => {
          const provider = providers.get(source.id);
          if (!provider) return null;
          const configured = provider.isConfigured();
          const h = health[source.id];
          const cooling = Boolean(h && h.downUntil > now);
          const name = provider.label;

          let status: { tone: 'success' | 'warning' | 'muted'; text: string };
          if (!configured) status = { tone: 'muted', text: uiI18n.t('metadataSources.statusNotConfigured') };
          else if (cooling && h) {
            status = {
              tone: 'warning',
              text: h.lastError
                ? uiI18n.t('metadataSources.statusCoolingError', { time: formatTime(h.downUntil), error: h.lastError })
                : uiI18n.t('metadataSources.statusCooling', { time: formatTime(h.downUntil) }),
            };
          } else status = { tone: 'success', text: uiI18n.t('metadataSources.statusOk') };

          return (
            <li key={source.id}>
              <SettingRow
                title={name}
                description={
                  <>
                    {provider.description}
                    {cooling && h?.lastError && <span className="mt-1 block text-xs">{status.text}</span>}
                  </>
                }
                badge={
                  <>
                    {source.id === primaryId && source.enabled && (
                      <SettingsBadge tone="primary">{uiI18n.t('metadataSources.primary')}</SettingsBadge>
                    )}
                    <SettingsBadge tone={status.tone}>
                      {!configured
                        ? uiI18n.t('metadataSources.statusNotConfigured')
                        : cooling
                          ? uiI18n.t('metadataSources.statusCooling', { time: formatTime(h!.downUntil) })
                          : uiI18n.t('metadataSources.statusOk')}
                    </SettingsBadge>
                  </>
                }
                control={({ labelId }) => (
                  <div className="flex items-center gap-1.5">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      disabled={index === 0}
                      onClick={() => move(index, -1)}
                      aria-label={uiI18n.t('metadataSources.moveUp', { name })}
                    >
                      <ArrowUp className="h-4 w-4" />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      disabled={index === sources.length - 1}
                      onClick={() => move(index, 1)}
                      aria-label={uiI18n.t('metadataSources.moveDown', { name })}
                    >
                      <ArrowDown className="h-4 w-4" />
                    </Button>
                    <Switch
                      checked={source.enabled}
                      onCheckedChange={(v) => toggle(source.id, v)}
                      aria-labelledby={labelId}
                    />
                  </div>
                )}
              />
            </li>
          );
        })}
      </ol>
    </SettingsSection>
  );
}
