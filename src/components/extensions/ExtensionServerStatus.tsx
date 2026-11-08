import uiI18n from "@/lib/uiI18n";
import { useUiTranslation } from "@/hooks/ui/useUiTranslation";
import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Server, Settings2, RefreshCw } from "lucide-react";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ExtensionLogo } from "./ExtensionLogo";
import {
  listServers,
  serverRequest,
  type ServerConnection,
  type ServerHealth,
} from "@/core/extensions/server/client";
import { EXTENSION_INSTALLED_EVENT } from "@/core/extensions/ecosystems";
import { formatServiceDuration } from "@/lib/betterStackStatus";
import { useSettingsModal } from "@/contexts/SettingsModalContext";

function LinkedServer({ server }: { server: ServerConnection }) {
  useUiTranslation();
  const health = useQuery({
    queryKey: [
      "extension-server-health",
      server.id,
      server.url,
      server.enabled,
    ],
    enabled: server.enabled,
    queryFn: ({ signal }) =>
      serverRequest<ServerHealth>(server, "/health", undefined, signal),
    staleTime: 30000,
    refetchInterval: 60000,
    refetchIntervalInBackground: false,
    retry: 0,
  });
  const compatible =
    health.data?.protocolVersion === 1 &&
    health.data.serverId === server.serverId;
  const online = server.enabled && !health.isError && compatible;
  const state = !server.enabled
    ? uiI18n.t("extensionServerClient.statusDisabled")
    : health.isFetching && !health.data
    ? uiI18n.t("extensionServerClient.statusChecking")
    : health.isError
    ? uiI18n.t("extensionServerClient.offline")
    : compatible
    ? uiI18n.t("extensionServerClient.online")
    : health.data
    ? uiI18n.t("extensionServerClient.statusIncompatible")
    : uiI18n.t("extensionServerClient.statusUnknown");
  // Prefer the local name: the user may have renamed the PC on this device.
  const name = server.name || health.data?.name || server.url;
  let host = server.url;
  try { host = new URL(server.url).host; } catch { /* Shown as stored. */ }
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-xl border border-border p-4">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <ExtensionLogo name={name} src={health.data?.logoUrl || server.logoUrl} />
          <div className="min-w-0">
            <h3 className="truncate font-semibold">{name}</h3>
            <p className="truncate text-xs text-muted-foreground">{host}</p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <Badge variant={online ? "secondary" : "outline"}>{state}</Badge>
          {online && health.data?.uptimeSeconds !== undefined && (
            <span className="text-muted-foreground">
              {uiI18n.t("extensionServerClient.runningFor", { time: formatServiceDuration(health.data.uptimeSeconds) })}
            </span>
          )}
          <Button
            size="icon"
            variant="ghost"
            disabled={!server.enabled || health.isFetching}
            aria-label={uiI18n.t("extensionServerClient.refreshStatus", { name })}
            onClick={() => health.refetch()}
          >
            <RefreshCw className={health.isFetching ? "animate-spin" : ""} />
          </Button>
        </div>
      </div>
      {server.enabled && health.isError && (
        <p role="status" className="text-xs text-muted-foreground">
          {health.error instanceof Error ? health.error.message : ""} {uiI18n.t("extensionServerClient.offlineFallback")}
        </p>
      )}
    </div>
  );
}

export function ExtensionServerStatus() {
  useUiTranslation();
  const [servers, setServers] = useState(listServers);
  const { openSettings } = useSettingsModal();
  useEffect(() => {
    const refresh = () => setServers(listServers());
    window.addEventListener(EXTENSION_INSTALLED_EVENT, refresh);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener(EXTENSION_INSTALLED_EVENT, refresh);
      window.removeEventListener("storage", refresh);
    };
  }, []);
  return (
    <Card className="mt-6">
      <CardHeader className="flex-row flex-wrap items-center justify-between gap-3">
        <div>
          <CardTitle className="flex items-center gap-2">
            <Server className="size-5" />
            {uiI18n.t("extensionServerClient.serversTitle")}
          </CardTitle>
          <CardDescription>
            {servers.length
              ? uiI18n.t("extensionServerClient.statusDescription")
              : uiI18n.t("extensionServerClient.emptyStatus")}
          </CardDescription>
        </div>
        <Button variant="outline" onClick={() => openSettings("extensions")}>
          <Settings2 />
          {uiI18n.t("extensionServerClient.manage")}
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {servers.length ? (
          servers.map((server) => (
            <LinkedServer key={server.id} server={server} />
          ))
        ) : (
          <p className="text-sm text-muted-foreground">
            {uiI18n.t("extensionServerClient.noServers")}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
