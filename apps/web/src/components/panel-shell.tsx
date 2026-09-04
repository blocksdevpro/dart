"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import type { LucideIcon } from "lucide-react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  ChevronDown,
  CircleHelp,
  Copy,
  Globe2,
  HardDrive,
  Loader2,
  PackagePlus,
  Play,
  Plus,
  Puzzle,
  RefreshCw,
  Search,
  Server,
  Settings,
  ShieldCheck,
  Square,
  Terminal,
  Trash2,
  UserRound,
  Users,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DartApi,
  type ConsoleLine,
  type ContentKind,
  type ContentSearchHit,
  type CreateInstanceOptions,
  type DartInstance,
  type HealthResponse,
  type InstalledContent,
  type InstanceSize,
  type InstanceStatus,
  type SystemInfo,
  readableError,
} from "@/lib/dart-api";
import { cn } from "@/lib/utils";

type Screen = "servers" | "server" | "settings";
type ServerTab = "overview" | "addons" | "console";
type ConnectionState = "connecting" | "online" | "offline";

const panelClass =
  "rounded-2xl border border-[var(--line)] bg-[var(--panel)] shadow-[0_24px_80px_rgba(0,0,0,.14)]";

const contentLabels: Record<ContentKind, { singular: string; plural: string }> = {
  mod: { singular: "mod", plural: "Mods" },
  data_pack: { singular: "data pack", plural: "Data packs" },
  resource_pack: { singular: "resource pack", plural: "Resource packs" },
};

const serverTabs: Array<{ value: ServerTab; label: string; icon: LucideIcon }> = [
  { value: "overview", label: "Overview", icon: Server },
  { value: "addons", label: "Add-ons", icon: Puzzle },
  { value: "console", label: "Console", icon: Terminal },
];

function statusDetails(status: InstanceStatus) {
  switch (status) {
    case "running":
      return { label: "Online", detail: "Players can join", tone: "success" as const };
    case "starting":
      return { label: "Starting", detail: "Getting the world ready", tone: "warning" as const };
    case "stopping":
      return { label: "Stopping", detail: "Saving the world", tone: "warning" as const };
    case "failed":
      return { label: "Needs attention", detail: "Dart could not start it", tone: "danger" as const };
    case "stopped":
      return { label: "Offline", detail: "Start it whenever you are ready", tone: "neutral" as const };
  }
}

function formatMemory(mib: number) {
  return mib >= 1024 ? `${Number((mib / 1024).toFixed(1))} GB` : `${mib} MB`;
}

function sizeLabel(mib: number) {
  if (mib <= 2048) return "Personal";
  if (mib <= 4096) return "Friends";
  return "Community";
}

function formatDownloads(downloads: number) {
  return new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(downloads);
}

function formatUptime(seconds: number) {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function StatusDot({ status }: { status: InstanceStatus }) {
  return (
    <span
      className={cn(
        "inline-block size-2 shrink-0 rounded-full",
        status === "running" && "bg-emerald-400 shadow-[0_0_12px_rgba(52,211,153,.65)]",
        (status === "starting" || status === "stopping") && "animate-pulse bg-amber-300",
        status === "failed" && "bg-red-400",
        status === "stopped" && "bg-[var(--ink-faint)]",
      )}
      aria-hidden="true"
    />
  );
}

function DartMark({ compact = false }: { compact?: boolean }) {
  return (
    <span
      className={cn(
        "relative grid shrink-0 place-items-center overflow-hidden rounded-[11px] bg-[var(--signal)] font-black text-[var(--signal-ink)] shadow-[0_0_30px_rgba(216,255,107,.13)]",
        compact ? "size-9 text-base" : "size-11 text-lg",
      )}
      aria-hidden="true"
    >
      <span className="relative z-10 -translate-x-px">D</span>
      <span className="absolute -bottom-3 -right-2 size-7 rotate-45 rounded-sm bg-black/12" />
    </span>
  );
}

function Select({ className, ...props }: React.ComponentProps<"select">) {
  return (
    <span className={cn("relative inline-flex", className)}>
      <select
        className="h-11 w-full appearance-none rounded-xl border border-[var(--line)] bg-[var(--panel-deep)] py-0 pl-3.5 pr-10 text-sm font-medium text-[var(--ink)] outline-none transition focus:border-[var(--signal)]/45 focus:ring-2 focus:ring-[var(--signal)]/10"
        {...props}
      />
      <ChevronDown className="pointer-events-none absolute right-3 top-1/2 size-4 -translate-y-1/2 text-[var(--ink-faint)]" />
    </span>
  );
}

function EmptyState({
  icon: Icon,
  title,
  detail,
  action,
}: {
  icon: LucideIcon;
  title: string;
  detail: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="grid min-h-72 place-items-center px-6 py-12 text-center">
      <div className="max-w-sm">
        <span className="mx-auto grid size-12 place-items-center rounded-2xl border border-[var(--line)] bg-[var(--panel-raised)] text-[var(--signal)]">
          <Icon className="size-5" />
        </span>
        <h2 className="mt-5 text-base font-semibold tracking-[-0.02em]">{title}</h2>
        <p className="mt-2 text-sm leading-6 text-[var(--ink-muted)]">{detail}</p>
        {action ? <div className="mt-6">{action}</div> : null}
      </div>
    </div>
  );
}

export function PanelShell() {
  const [screen, setScreen] = useState<Screen>("servers");
  const [serverTab, setServerTab] = useState<ServerTab>("overview");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("connecting");
  const [apiBase, setApiBase] = useState(() =>
    typeof window === "undefined"
      ? "/dart-api"
      : window.localStorage.getItem("dart-api-base") || "/dart-api",
  );
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const [instances, setInstances] = useState<DartInstance[]>([]);
  const [createOpen, setCreateOpen] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [creationOptions, setCreationOptions] = useState<CreateInstanceOptions | null>(null);
  const [optionsLoading, setOptionsLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState<string | null>(null);

  const api = useMemo(() => new DartApi(apiBase), [apiBase]);
  const selectedInstance = instances.find((instance) => instance.id === selectedId);

  const loadCore = useCallback(
    async (showError = false) => {
      try {
        const [nextHealth, nextSystem, nextInstances] = await Promise.all([
          api.health(),
          api.system(),
          api.listInstances(),
        ]);
        setHealth(nextHealth);
        setSystem(nextSystem);
        setInstances(nextInstances);
        setConnection("online");
      } catch (error) {
        setConnection("offline");
        if (showError) toast.error(readableError(error));
      }
    },
    [api],
  );

  const loadCreationOptions = useCallback(async () => {
    setOptionsLoading(true);
    try {
      setCreationOptions(await api.creationOptions());
    } catch (error) {
      setCreationOptions(null);
      toast.error(readableError(error));
    } finally {
      setOptionsLoading(false);
    }
  }, [api]);

  useEffect(() => {
    const initial = window.setTimeout(() => void loadCore(), 0);
    const interval = window.setInterval(() => void loadCore(), 5_000);
    return () => {
      window.clearTimeout(initial);
      window.clearInterval(interval);
    };
  }, [loadCore]);

  useEffect(() => {
    if (createOpen && !creationOptions && !optionsLoading && connection === "online") {
      const timer = window.setTimeout(() => void loadCreationOptions(), 0);
      return () => window.clearTimeout(timer);
    }
  }, [connection, createOpen, creationOptions, loadCreationOptions, optionsLoading]);

  const openServer = (id: string, tab: ServerTab = "overview") => {
    setSelectedId(id);
    setServerTab(tab);
    setScreen("server");
  };

  const goToServers = () => {
    setScreen("servers");
    setServerTab("overview");
  };

  const runAction = async (
    instance: DartInstance,
    action: "start" | "stop" | "restart",
  ) => {
    if (connection !== "online") {
      toast.info("Dart is offline. Reconnect before changing a server.");
      return;
    }
    setActionBusy(instance.id);
    try {
      await api.instanceAction(instance.id, action);
      const verb = action === "start" ? "started" : action === "stop" ? "stopped" : "restarted";
      toast.success(`${instance.name} ${verb}`);
      await loadCore();
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setActionBusy(null);
    }
  };

  const createServer = async (payload: {
    name: string;
    minecraft: string;
    size: InstanceSize;
    accept_eula: boolean;
  }) => {
    setCreateBusy(true);
    try {
      const created = await api.createInstance(payload);
      setCreateOpen(false);
      await loadCore();
      openServer(created.id);
      toast.success(`${created.name} is ready`);
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setCreateBusy(false);
    }
  };

  const openCreate = () => {
    if (connection !== "online") {
      toast.info("Start or reconnect Dart before creating a server.");
      return;
    }
    setCreateOpen(true);
  };

  return (
    <div className="min-h-screen bg-[var(--canvas)] text-[var(--ink)]">
      <aside className="fixed inset-y-0 left-0 z-40 hidden w-60 border-r border-[var(--line)] bg-[var(--sidebar)] lg:flex lg:flex-col">
        <button
          type="button"
          onClick={goToServers}
          className="flex items-center gap-3 px-5 py-5 text-left"
          aria-label="Go to servers"
        >
          <DartMark />
          <span>
            <span className="block text-sm font-semibold tracking-[-0.02em]">Dart</span>
            <span className="mt-0.5 block text-[9px] font-semibold uppercase tracking-[0.18em] text-[var(--ink-faint)]">
              Server control
            </span>
          </span>
        </button>

        <nav className="mt-6 px-3" aria-label="Main navigation">
          <button
            type="button"
            onClick={goToServers}
            className={cn(
              "flex h-11 w-full items-center gap-3 rounded-xl px-3 text-sm font-medium transition",
              screen !== "settings"
                ? "bg-[var(--panel-raised)] text-[var(--ink)]"
                : "text-[var(--ink-muted)] hover:bg-[var(--panel-raised)] hover:text-[var(--ink)]",
            )}
          >
            <Server className="size-4" />
            Servers
            <span className="ml-auto rounded-full bg-black/20 px-2 py-0.5 text-[10px] text-[var(--ink-muted)]">
              {instances.length}
            </span>
          </button>
        </nav>

        <div className="mt-auto space-y-2 p-3">
          <button
            type="button"
            onClick={() => setScreen("settings")}
            className={cn(
              "flex h-10 w-full items-center gap-3 rounded-xl px-3 text-sm transition",
              screen === "settings"
                ? "bg-[var(--panel-raised)] text-[var(--ink)]"
                : "text-[var(--ink-muted)] hover:bg-[var(--panel-raised)] hover:text-[var(--ink)]",
            )}
          >
            <Settings className="size-4" />
            Settings
          </button>
          <div className="rounded-xl border border-[var(--line)] bg-[var(--panel)] px-3 py-3">
            <div className="flex items-center gap-2.5">
              {connection === "online" ? (
                <Wifi className="size-4 text-emerald-400" />
              ) : connection === "connecting" ? (
                <Loader2 className="size-4 animate-spin text-[var(--ink-faint)]" />
              ) : (
                <WifiOff className="size-4 text-red-300" />
              )}
              <div>
                <p className="text-xs font-semibold">
                  {connection === "online"
                    ? "Dart is ready"
                    : connection === "connecting"
                      ? "Connecting"
                      : "Dart is offline"}
                </p>
                <p className="mt-0.5 text-[10px] text-[var(--ink-faint)]">
                  {connection === "online" ? `Daemon ${health?.version ?? ""}` : "Check the connection"}
                </p>
              </div>
            </div>
          </div>
        </div>
      </aside>

      <div className="lg:pl-60">
        <header className="sticky top-0 z-30 flex h-16 items-center justify-between border-b border-[var(--line)] bg-[var(--canvas)]/90 px-4 backdrop-blur-xl sm:px-6 lg:px-8">
          <button
            type="button"
            className="flex items-center gap-2.5 lg:hidden"
            onClick={goToServers}
            aria-label="Go to servers"
          >
            <DartMark compact />
            <span className="text-sm font-semibold">Dart</span>
          </button>
          <div className="hidden items-center gap-2 text-xs text-[var(--ink-faint)] lg:flex">
            <button type="button" onClick={goToServers} className="transition hover:text-[var(--ink)]">
              Servers
            </button>
            {screen === "server" && selectedInstance ? (
              <>
                <span>/</span>
                <span className="text-[var(--ink-muted)]">{selectedInstance.name}</span>
              </>
            ) : null}
            {screen === "settings" ? (
              <>
                <span>/</span>
                <span className="text-[var(--ink-muted)]">Settings</span>
              </>
            ) : null}
          </div>
          <div className="ml-auto flex items-center gap-2">
            <span className="hidden sm:inline-flex">
              <Badge tone={connection === "online" ? "success" : "danger"}>
                {connection === "online" ? "Connected" : "Offline"}
              </Badge>
            </span>
            <Button
              variant="ghost"
              size="icon-sm"
              className="lg:hidden"
              onClick={() => setScreen("settings")}
              aria-label="Open settings"
            >
              <Settings />
            </Button>
            <Button size="sm" onClick={openCreate} disabled={connection !== "online"}>
              <Plus />
              New server
            </Button>
          </div>
        </header>

        <main className="mx-auto w-full max-w-[1180px] px-4 py-8 sm:px-6 sm:py-10 lg:px-10 lg:py-12">
          {screen === "servers" ? (
            <ServersView
              connection={connection}
              instances={instances}
              actionBusy={actionBusy}
              onOpen={openServer}
              onAction={runAction}
              onCreate={openCreate}
              onReconnect={() => void loadCore(true)}
            />
          ) : null}

          {screen === "server" && selectedInstance ? (
            <ServerView
              api={api}
              connection={connection}
              instance={selectedInstance}
              tab={serverTab}
              actionBusy={actionBusy === selectedInstance.id}
              onBack={goToServers}
              onTab={setServerTab}
              onAction={(action) => void runAction(selectedInstance, action)}
            />
          ) : null}

          {screen === "server" && !selectedInstance ? (
            <EmptyState
              icon={Server}
              title="That server is not available"
              detail="It may have been moved or removed. Return to your server list and choose another one."
              action={<Button onClick={goToServers}>Back to servers</Button>}
            />
          ) : null}

          {screen === "settings" ? (
            <SettingsView
              apiBase={apiBase}
              connection={connection}
              health={health}
              system={system}
              onSave={(nextBase) => {
                window.localStorage.setItem("dart-api-base", nextBase);
                setApiBase(nextBase);
                setConnection("connecting");
              }}
              onReconnect={() => void loadCore(true)}
            />
          ) : null}
        </main>
      </div>

      <NewServerWizard
        key={createOpen ? "wizard-open" : "wizard-closed"}
        open={createOpen}
        busy={createBusy}
        loading={optionsLoading}
        options={creationOptions}
        onClose={() => setCreateOpen(false)}
        onRetry={() => void loadCreationOptions()}
        onCreate={createServer}
      />
    </div>
  );
}

function ServersView({
  connection,
  instances,
  actionBusy,
  onOpen,
  onAction,
  onCreate,
  onReconnect,
}: {
  connection: ConnectionState;
  instances: DartInstance[];
  actionBusy: string | null;
  onOpen: (id: string, tab?: ServerTab) => void;
  onAction: (
    instance: DartInstance,
    action: "start" | "stop" | "restart",
  ) => Promise<void>;
  onCreate: () => void;
  onReconnect: () => void;
}) {
  const onlineCount = instances.filter((instance) => instance.state.status === "running").length;

  return (
    <div>
      <div className="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--signal)]">
            Server home
          </p>
          <h1 className="mt-2 text-3xl font-semibold tracking-[-0.045em] sm:text-4xl">
            Your servers
          </h1>
          <p className="mt-2 text-sm text-[var(--ink-muted)]">
            Start a world, manage its add-ons, or make a new one.
          </p>
        </div>
        {instances.length > 0 ? (
          <div className="flex items-center gap-2 text-xs text-[var(--ink-muted)]">
            <span className="size-2 rounded-full bg-emerald-400" />
            {onlineCount} online · {instances.length} total
          </div>
        ) : null}
      </div>

      {connection === "offline" ? (
        <div className="mt-7 flex flex-col gap-4 rounded-2xl border border-red-400/18 bg-red-400/[0.055] p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-start gap-3">
            <WifiOff className="mt-0.5 size-5 shrink-0 text-red-300" />
            <div>
              <p className="text-sm font-semibold">Dart is not connected</p>
              <p className="mt-1 text-xs leading-5 text-[var(--ink-muted)]">
                Start the daemon, then reconnect. Your servers are not changed while Dart is offline.
              </p>
            </div>
          </div>
          <Button variant="secondary" size="sm" onClick={onReconnect}>
            <RefreshCw /> Reconnect
          </Button>
        </div>
      ) : null}

      {connection === "connecting" && instances.length === 0 ? (
        <div className="mt-8 grid min-h-72 place-items-center">
          <div className="text-center text-sm text-[var(--ink-muted)]">
            <Loader2 className="mx-auto mb-3 size-5 animate-spin text-[var(--signal)]" />
            Loading your servers…
          </div>
        </div>
      ) : null}

      {connection !== "connecting" && instances.length === 0 ? (
        <div className={cn(panelClass, "mt-8")}>
          <EmptyState
            icon={Server}
            title={connection === "online" ? "Make your first server" : "No servers to show yet"}
            detail={
              connection === "online"
                ? "Pick a name, a Minecraft release, and a size. Dart handles the technical setup for you."
                : "Reconnect Dart to load your server list."
            }
            action={
              connection === "online" ? (
                <Button onClick={onCreate}>
                  <Plus /> Create a server
                </Button>
              ) : null
            }
          />
        </div>
      ) : null}

      {instances.length > 0 ? (
        <div className="mt-8 grid gap-4 md:grid-cols-2">
          {instances.map((instance) => (
            <ServerCard
              key={instance.id}
              instance={instance}
              busy={actionBusy === instance.id}
              onOpen={() => onOpen(instance.id)}
              onAction={(action) => void onAction(instance, action)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ServerCard({
  instance,
  busy,
  onOpen,
  onAction,
}: {
  instance: DartInstance;
  busy: boolean;
  onOpen: () => void;
  onAction: (action: "start" | "stop") => void;
}) {
  const status = statusDetails(instance.state.status);
  const transitioning = instance.state.status === "starting" || instance.state.status === "stopping";
  const running = instance.state.status === "running";

  return (
    <article className={cn(panelClass, "overflow-hidden transition hover:border-[var(--line-strong)]")}>
      <button type="button" onClick={onOpen} className="block w-full p-5 text-left sm:p-6">
        <div className="flex items-start gap-4">
          <span className="grid size-11 shrink-0 place-items-center rounded-2xl border border-[var(--line)] bg-[var(--panel-raised)] text-[var(--signal)]">
            <Server className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="truncate text-base font-semibold tracking-[-0.025em]">{instance.name}</h2>
                <div className="mt-1.5 flex items-center gap-2 text-xs text-[var(--ink-muted)]">
                  <StatusDot status={instance.state.status} />
                  <span>{status.label}</span>
                  <span className="text-[var(--ink-faint)]">· {status.detail}</span>
                </div>
              </div>
              <ArrowRight className="mt-1 size-4 shrink-0 text-[var(--ink-faint)]" />
            </div>
            <div className="mt-6 grid grid-cols-2 gap-3">
              <div className="rounded-xl bg-[var(--panel-deep)] px-3.5 py-3">
                <p className="text-[10px] text-[var(--ink-faint)]">Minecraft</p>
                <p className="mt-1 text-sm font-semibold">{instance.config.fabric.minecraft}</p>
              </div>
              <div className="rounded-xl bg-[var(--panel-deep)] px-3.5 py-3">
                <p className="text-[10px] text-[var(--ink-faint)]">Size</p>
                <p className="mt-1 text-sm font-semibold">
                  {sizeLabel(instance.config.max_memory_mib)}
                </p>
              </div>
            </div>
          </div>
        </div>
      </button>
      <div className="flex items-center justify-between border-t border-[var(--line)] bg-[var(--panel-deep)] px-5 py-3 sm:px-6">
        <Button variant="ghost" size="sm" onClick={onOpen}>
          Manage
        </Button>
        <Button
          size="sm"
          variant={running ? "secondary" : "default"}
          disabled={busy || transitioning}
          onClick={() => onAction(running ? "stop" : "start")}
        >
          {busy || transitioning ? (
            <Loader2 className="animate-spin" />
          ) : running ? (
            <Square />
          ) : (
            <Play />
          )}
          {running ? "Stop" : transitioning ? status.label : "Start"}
        </Button>
      </div>
    </article>
  );
}

function ServerView({
  api,
  connection,
  instance,
  tab,
  actionBusy,
  onBack,
  onTab,
  onAction,
}: {
  api: DartApi;
  connection: ConnectionState;
  instance: DartInstance;
  tab: ServerTab;
  actionBusy: boolean;
  onBack: () => void;
  onTab: (tab: ServerTab) => void;
  onAction: (action: "start" | "stop" | "restart") => void;
}) {
  const status = statusDetails(instance.state.status);
  const running = instance.state.status === "running";
  const transitioning = instance.state.status === "starting" || instance.state.status === "stopping";

  return (
    <div>
      <button
        type="button"
        onClick={onBack}
        className="mb-5 inline-flex items-center gap-2 text-xs font-medium text-[var(--ink-muted)] transition hover:text-[var(--ink)]"
      >
        <ArrowLeft className="size-4" /> Back to servers
      </button>

      <div className="flex flex-col gap-5 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 items-start gap-4">
          <span className="grid size-12 shrink-0 place-items-center rounded-2xl border border-[var(--line)] bg-[var(--panel-raised)] text-[var(--signal)]">
            <Server className="size-5" />
          </span>
          <div className="min-w-0">
            <h1 className="truncate text-3xl font-semibold tracking-[-0.045em]">{instance.name}</h1>
            <div className="mt-2 flex items-center gap-2 text-sm text-[var(--ink-muted)]">
              <StatusDot status={instance.state.status} />
              <span>{status.label}</span>
              <span className="text-[var(--ink-faint)]">· {status.detail}</span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {running ? (
            <Button variant="secondary" onClick={() => onAction("restart")} disabled={actionBusy}>
              <RefreshCw /> Restart
            </Button>
          ) : null}
          <Button
            variant={running ? "secondary" : "default"}
            onClick={() => onAction(running ? "stop" : "start")}
            disabled={actionBusy || transitioning || connection !== "online"}
          >
            {actionBusy || transitioning ? (
              <Loader2 className="animate-spin" />
            ) : running ? (
              <Square />
            ) : (
              <Play />
            )}
            {running ? "Stop server" : transitioning ? status.label : "Start server"}
          </Button>
        </div>
      </div>

      {instance.state.status === "failed" ? (
        <div className="mt-6 rounded-xl border border-red-400/20 bg-red-400/[0.06] px-4 py-3 text-sm text-red-200">
          {instance.state.message}
        </div>
      ) : null}

      <div className="mt-8 flex gap-1 overflow-x-auto border-b border-[var(--line)]" role="tablist">
        {serverTabs.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.value}
              type="button"
              role="tab"
              aria-selected={tab === item.value}
              onClick={() => onTab(item.value)}
              className={cn(
                "relative flex h-11 shrink-0 items-center gap-2 px-4 text-sm font-medium transition",
                tab === item.value
                  ? "text-[var(--ink)] after:absolute after:inset-x-3 after:bottom-0 after:h-0.5 after:rounded-full after:bg-[var(--signal)]"
                  : "text-[var(--ink-muted)] hover:text-[var(--ink)]",
              )}
            >
              <Icon className="size-4" /> {item.label}
            </button>
          );
        })}
      </div>

      <div className="mt-6">
        {tab === "overview" ? (
          <ServerOverview instance={instance} onTab={onTab} />
        ) : null}
        {tab === "addons" ? <AddonsView api={api} instance={instance} /> : null}
        {tab === "console" ? <ConsoleView api={api} instance={instance} /> : null}
      </div>
    </div>
  );
}

function ServerOverview({
  instance,
  onTab,
}: {
  instance: DartInstance;
  onTab: (tab: ServerTab) => void;
}) {
  return (
    <div className="grid gap-4 lg:grid-cols-[1.2fr_.8fr]">
      <section className={cn(panelClass, "p-5 sm:p-6")}>
        <h2 className="text-sm font-semibold">Server setup</h2>
        <p className="mt-1 text-xs text-[var(--ink-muted)]">The important details, without the plumbing.</p>
        <div className="mt-6 grid gap-3 sm:grid-cols-3">
          <InfoTile label="Minecraft" value={instance.config.fabric.minecraft} />
          <InfoTile label="Server size" value={sizeLabel(instance.config.max_memory_mib)} />
          <InfoTile label="Memory" value={formatMemory(instance.config.max_memory_mib)} />
        </div>
        <div className="mt-5 rounded-xl border border-[var(--line)] bg-[var(--panel-deep)] px-4 py-3.5">
          <div className="flex items-start gap-3">
            <ShieldCheck className="mt-0.5 size-4 shrink-0 text-[var(--signal)]" />
            <div>
              <p className="text-xs font-semibold">Fabric is managed automatically</p>
              <p className="mt-1 text-xs leading-5 text-[var(--ink-muted)]">
                Dart chose compatible launcher components for this Minecraft release. You do not need to update them by hand.
              </p>
            </div>
          </div>
        </div>
      </section>

      <section className={cn(panelClass, "overflow-hidden")}>
        <div className="border-b border-[var(--line)] px-5 py-4">
          <h2 className="text-sm font-semibold">What do you want to do?</h2>
        </div>
        <button
          type="button"
          onClick={() => onTab("addons")}
          className="flex w-full items-center gap-3 border-b border-[var(--line)] px-5 py-4 text-left transition hover:bg-[var(--panel-hover)]"
        >
          <span className="grid size-9 place-items-center rounded-xl bg-[var(--signal)]/10 text-[var(--signal)]">
            <Puzzle className="size-4" />
          </span>
          <span className="flex-1">
            <span className="block text-sm font-semibold">Manage add-ons</span>
            <span className="mt-0.5 block text-xs text-[var(--ink-faint)]">Find and install compatible mods</span>
          </span>
          <ArrowRight className="size-4 text-[var(--ink-faint)]" />
        </button>
        <button
          type="button"
          onClick={() => onTab("console")}
          className="flex w-full items-center gap-3 px-5 py-4 text-left transition hover:bg-[var(--panel-hover)]"
        >
          <span className="grid size-9 place-items-center rounded-xl bg-[var(--panel-raised)] text-[var(--ink-muted)]">
            <Terminal className="size-4" />
          </span>
          <span className="flex-1">
            <span className="block text-sm font-semibold">View console</span>
            <span className="mt-0.5 block text-xs text-[var(--ink-faint)]">See logs or send a command</span>
          </span>
          <ArrowRight className="size-4 text-[var(--ink-faint)]" />
        </button>
      </section>
    </div>
  );
}

function InfoTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-[var(--line)] bg-[var(--panel-raised)] p-4">
      <p className="text-[10px] text-[var(--ink-faint)]">{label}</p>
      <p className="mt-1.5 text-sm font-semibold">{value}</p>
    </div>
  );
}

function AddonsView({ api, instance }: { api: DartApi; instance: DartInstance }) {
  const [kind, setKind] = useState<ContentKind>("mod");
  const [mode, setMode] = useState<"installed" | "browse">("installed");
  const [installed, setInstalled] = useState<InstalledContent[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ContentSearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const stopped = instance.state.status === "stopped" || instance.state.status === "failed";

  const loadInstalled = useCallback(async () => {
    setLoading(true);
    try {
      setInstalled(await api.listContent(instance.id, kind));
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setLoading(false);
    }
  }, [api, instance.id, kind]);

  useEffect(() => {
    const timer = window.setTimeout(() => void loadInstalled(), 0);
    return () => window.clearTimeout(timer);
  }, [loadInstalled]);

  const search = async (event: FormEvent) => {
    event.preventDefault();
    const clean = query.trim();
    if (!clean) return;
    setSearching(true);
    try {
      setResults(await api.searchContent(instance.id, clean, kind));
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setSearching(false);
    }
  };

  const install = async (result: ContentSearchHit) => {
    setInstalling(result.id);
    try {
      const report = await api.installContent(instance.id, result.id, kind);
      const suffix = report.dependencies.length
        ? ` with ${report.dependencies.length} required ${report.dependencies.length === 1 ? "dependency" : "dependencies"}`
        : "";
      toast.success(`${report.title} installed${suffix}`);
      await loadInstalled();
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setInstalling(null);
    }
  };

  const remove = async (item: InstalledContent) => {
    if (!window.confirm(`Remove ${item.name} from ${instance.name}?`)) return;
    setRemoving(item.key);
    try {
      await api.removeContent(instance.id, item.key, kind);
      toast.success(`${item.name} removed`);
      await loadInstalled();
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setRemoving(null);
    }
  };

  const installedProjects = new Set(
    installed.flatMap((item) => (item.project_id ? [item.project_id] : [])),
  );

  return (
    <div className="space-y-4">
      {!stopped ? (
        <div className="flex items-start gap-3 rounded-xl border border-amber-300/20 bg-amber-300/[0.055] px-4 py-3.5">
          <CircleHelp className="mt-0.5 size-4 shrink-0 text-amber-300" />
          <div>
            <p className="text-xs font-semibold text-amber-100">Stop the server before changing add-ons</p>
            <p className="mt-1 text-xs text-[var(--ink-muted)]">You can still browse while it is online.</p>
          </div>
        </div>
      ) : null}

      <div className={cn(panelClass, "overflow-hidden")}>
        <div className="flex flex-col gap-3 border-b border-[var(--line)] p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex gap-1 rounded-xl bg-[var(--panel-deep)] p-1">
            {(["mod", "data_pack", "resource_pack"] as const).map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => {
                  setKind(value);
                  setResults([]);
                }}
                className={cn(
                  "rounded-lg px-3 py-2 text-xs font-medium transition",
                  kind === value
                    ? "bg-[var(--panel-raised)] text-[var(--ink)] shadow-sm"
                    : "text-[var(--ink-muted)] hover:text-[var(--ink)]",
                )}
              >
                {contentLabels[value].plural}
              </button>
            ))}
          </div>
          <div className="flex gap-1 rounded-xl border border-[var(--line)] p-1">
            <button
              type="button"
              onClick={() => setMode("installed")}
              className={cn(
                "rounded-lg px-3 py-1.5 text-xs font-medium transition",
                mode === "installed" ? "bg-[var(--panel-raised)] text-[var(--ink)]" : "text-[var(--ink-muted)]",
              )}
            >
              Installed
            </button>
            <button
              type="button"
              onClick={() => setMode("browse")}
              className={cn(
                "rounded-lg px-3 py-1.5 text-xs font-medium transition",
                mode === "browse" ? "bg-[var(--panel-raised)] text-[var(--ink)]" : "text-[var(--ink-muted)]",
              )}
            >
              Find add-ons
            </button>
          </div>
        </div>

        {mode === "installed" ? (
          loading ? (
            <div className="grid min-h-60 place-items-center text-sm text-[var(--ink-muted)]">
              <Loader2 className="size-5 animate-spin text-[var(--signal)]" />
            </div>
          ) : installed.length > 0 ? (
            <div className="divide-y divide-[var(--line)]">
              {installed.map((item) => (
                <div key={item.key} className="flex items-center gap-3 px-4 py-4 sm:px-5">
                  <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[var(--panel-raised)] text-[var(--signal)]">
                    <Puzzle className="size-4" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold">{item.name}</p>
                    <p className="mt-1 truncate text-xs text-[var(--ink-faint)]">
                      {item.version ? `Version ${item.version}` : item.file_name}
                    </p>
                  </div>
                  {item.managed ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!stopped || removing === item.key}
                      onClick={() => void remove(item)}
                      aria-label={`Remove ${item.name}`}
                    >
                      {removing === item.key ? <Loader2 className="animate-spin" /> : <Trash2 />}
                      <span className="hidden sm:inline">Remove</span>
                    </Button>
                  ) : (
                    <Badge tone="neutral">Added manually</Badge>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <EmptyState
              icon={Puzzle}
              title={`No ${contentLabels[kind].plural.toLowerCase()} yet`}
              detail="Find compatible add-ons and Dart will install the right release for this server."
              action={
                <Button variant="secondary" onClick={() => setMode("browse")}>
                  <Search /> Find add-ons
                </Button>
              }
            />
          )
        ) : (
          <div>
            <form onSubmit={search} className="flex flex-col gap-2 border-b border-[var(--line)] p-4 sm:flex-row">
              <label className="relative flex-1">
                <Search className="absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-[var(--ink-faint)]" />
                <Input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder={`Search ${contentLabels[kind].plural.toLowerCase()}…`}
                  className="h-11 pl-10"
                />
              </label>
              <Button type="submit" disabled={searching || !query.trim()}>
                {searching ? <Loader2 className="animate-spin" /> : <Search />}
                Search
              </Button>
            </form>

            {results.length > 0 ? (
              <div className="grid gap-px bg-[var(--line)] md:grid-cols-2">
                {results.map((result) => {
                  const alreadyInstalled = installedProjects.has(result.id);
                  return (
                    <article key={result.id} className="bg-[var(--panel)] p-5">
                      <div className="flex items-start gap-3">
                        <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[var(--panel-raised)] text-sm font-bold text-[var(--signal)]">
                          {result.title.slice(0, 2).toUpperCase()}
                        </span>
                        <div className="min-w-0 flex-1">
                          <h3 className="truncate text-sm font-semibold">{result.title}</h3>
                          <p className="mt-1 text-[11px] text-[var(--ink-faint)]">
                            by {result.author} · {formatDownloads(result.downloads)} downloads
                          </p>
                        </div>
                      </div>
                      <p className="mt-4 line-clamp-2 min-h-10 text-xs leading-5 text-[var(--ink-muted)]">
                        {result.description}
                      </p>
                      <div className="mt-4 flex items-center justify-between gap-3">
                        <Badge tone="success">Compatible</Badge>
                        <Button
                          size="sm"
                          disabled={!stopped || alreadyInstalled || installing === result.id}
                          onClick={() => void install(result)}
                        >
                          {installing === result.id ? (
                            <Loader2 className="animate-spin" />
                          ) : alreadyInstalled ? (
                            <Check />
                          ) : (
                            <PackagePlus />
                          )}
                          {alreadyInstalled ? "Installed" : "Install"}
                        </Button>
                      </div>
                    </article>
                  );
                })}
              </div>
            ) : (
              <EmptyState
                icon={Search}
                title="Find something for your server"
                detail={`Search by name. Dart only shows ${contentLabels[kind].plural.toLowerCase()} compatible with Minecraft ${instance.config.fabric.minecraft}.`}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ConsoleView({ api, instance }: { api: DartApi; instance: DartInstance }) {
  const [logs, setLogs] = useState<ConsoleLine[]>([]);
  const [loading, setLoading] = useState(true);
  const [command, setCommand] = useState("");
  const [sending, setSending] = useState(false);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await api.logs(instance.id, 120));
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setLoading(false);
    }
  }, [api, instance.id]);

  useEffect(() => {
    const initial = window.setTimeout(() => void loadLogs(), 0);
    const interval = window.setInterval(() => void loadLogs(), 3_000);
    return () => {
      window.clearTimeout(initial);
      window.clearInterval(interval);
    };
  }, [loadLogs]);

  const send = async (event: FormEvent) => {
    event.preventDefault();
    const clean = command.trim();
    if (!clean || instance.state.status !== "running") return;
    setSending(true);
    try {
      await api.command(instance.id, clean);
      setCommand("");
      await loadLogs();
    } catch (error) {
      toast.error(readableError(error));
    } finally {
      setSending(false);
    }
  };

  return (
    <section className={cn(panelClass, "overflow-hidden")}>
      <div className="flex items-center justify-between border-b border-[var(--line)] px-4 py-3 sm:px-5">
        <div>
          <h2 className="text-sm font-semibold">Server console</h2>
          <p className="mt-1 text-xs text-[var(--ink-faint)]">Live messages and advanced commands</p>
        </div>
        <Button variant="ghost" size="sm" onClick={() => void loadLogs()} disabled={loading}>
          <RefreshCw className={cn(loading && "animate-spin")} /> Refresh
        </Button>
      </div>
      <div className="relative min-h-[430px] max-h-[560px] overflow-y-auto bg-[#080a09] p-4 font-mono text-[11px] leading-6 sm:p-5">
        <div className="pointer-events-none absolute inset-0 opacity-[0.035] [background-image:linear-gradient(rgba(255,255,255,.35)_1px,transparent_1px)] [background-size:100%_24px]" />
        {logs.length > 0 ? (
          <div className="relative space-y-0.5">
            {logs.map((line, index) => (
              <div
                key={`${line.timestamp_millis}-${index}`}
                className={cn(
                  "grid gap-3 sm:grid-cols-[68px_1fr]",
                  line.stream === "stderr" ? "text-red-300" : "text-[#aeb8b0]",
                )}
              >
                <span className="text-[#59615b]">
                  {new Date(line.timestamp_millis).toLocaleTimeString([], {
                    hour12: false,
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </span>
                <span className="break-words">{line.line}</span>
              </div>
            ))}
          </div>
        ) : (
          <div className="relative grid min-h-[390px] place-items-center text-center text-[#59615b]">
            <div>
              <Terminal className="mx-auto mb-3 size-5" />
              <p>{loading ? "Loading console…" : "No console messages yet."}</p>
            </div>
          </div>
        )}
      </div>
      <form onSubmit={send} className="flex items-center gap-2 border-t border-[var(--line)] bg-[var(--panel-deep)] p-3">
        <span className="pl-1 font-mono text-sm text-[var(--signal)]">›</span>
        <Input
          value={command}
          onChange={(event) => setCommand(event.target.value)}
          disabled={instance.state.status !== "running"}
          placeholder={
            instance.state.status === "running"
              ? "Type a server command…"
              : "Start the server to send commands"
          }
          className="border-0 bg-transparent font-mono shadow-none focus:ring-0"
        />
        <Button
          type="submit"
          size="sm"
          disabled={sending || instance.state.status !== "running" || !command.trim()}
        >
          {sending ? <Loader2 className="animate-spin" /> : <ArrowRight />} Send
        </Button>
      </form>
    </section>
  );
}

function SettingsView({
  apiBase,
  connection,
  health,
  system,
  onSave,
  onReconnect,
}: {
  apiBase: string;
  connection: ConnectionState;
  health: HealthResponse | null;
  system: SystemInfo | null;
  onSave: (base: string) => void;
  onReconnect: () => void;
}) {
  const [draft, setDraft] = useState(apiBase);

  const save = (event: FormEvent) => {
    event.preventDefault();
    const clean = draft.trim().replace(/\/$/, "") || "/dart-api";
    onSave(clean);
    toast.success("Connection saved");
  };

  const copy = async (value: string) => {
    await navigator.clipboard.writeText(value);
    toast.success("Copied");
  };

  return (
    <div>
      <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--signal)]">Dart</p>
      <h1 className="mt-2 text-3xl font-semibold tracking-[-0.045em]">Settings</h1>
      <p className="mt-2 text-sm text-[var(--ink-muted)]">Connection and daemon details.</p>

      <div className="mt-8 grid gap-4 lg:grid-cols-[1.05fr_.95fr]">
        <section className={cn(panelClass, "p-5 sm:p-6")}>
          <div className="flex items-start gap-3">
            <span
              className={cn(
                "grid size-10 shrink-0 place-items-center rounded-xl",
                connection === "online"
                  ? "bg-emerald-400/10 text-emerald-300"
                  : "bg-red-400/10 text-red-300",
              )}
            >
              {connection === "online" ? <Wifi className="size-4" /> : <WifiOff className="size-4" />}
            </span>
            <div className="flex-1">
              <h2 className="text-sm font-semibold">
                {connection === "online" ? "Dart is connected" : "Dart is not connected"}
              </h2>
              <p className="mt-1 text-xs leading-5 text-[var(--ink-muted)]">
                {connection === "online"
                  ? "The panel can create and manage servers."
                  : "Make sure the daemon is running, then try again."}
              </p>
            </div>
          </div>
          <Button className="mt-5" variant="secondary" onClick={onReconnect}>
            <RefreshCw /> Test connection
          </Button>

          <details className="mt-6 border-t border-[var(--line)] pt-5">
            <summary className="cursor-pointer text-xs font-semibold text-[var(--ink-muted)]">
              Advanced connection settings
            </summary>
            <form onSubmit={save} className="mt-4">
              <label className="text-[10px] font-semibold uppercase tracking-[0.12em] text-[var(--ink-faint)]">
                API address
              </label>
              <div className="mt-2 flex gap-2">
                <Input value={draft} onChange={(event) => setDraft(event.target.value)} />
                <Button type="submit">Save</Button>
              </div>
            </form>
          </details>
        </section>

        <section className={cn(panelClass, "overflow-hidden")}>
          <div className="border-b border-[var(--line)] px-5 py-4">
            <h2 className="text-sm font-semibold">About this daemon</h2>
          </div>
          <SettingsRow icon={Server} label="Version" value={health?.version ?? "Unavailable"} />
          <SettingsRow
            icon={RefreshCw}
            label="Uptime"
            value={health ? formatUptime(health.uptime_seconds) : "Unavailable"}
          />
          <SettingsRow
            icon={HardDrive}
            label="Data folder"
            value={system?.dart_home ?? "Unavailable"}
            onCopy={system ? () => void copy(system.dart_home) : undefined}
          />
        </section>
      </div>
    </div>
  );
}

function SettingsRow({
  icon: Icon,
  label,
  value,
  onCopy,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  onCopy?: () => void;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-[var(--line)] px-5 py-4 last:border-b-0">
      <Icon className="size-4 shrink-0 text-[var(--ink-faint)]" />
      <div className="min-w-0 flex-1">
        <p className="text-[10px] text-[var(--ink-faint)]">{label}</p>
        <p className="mt-1 truncate text-xs text-[var(--ink-muted)]">{value}</p>
      </div>
      {onCopy ? (
        <Button variant="ghost" size="icon-sm" onClick={onCopy} aria-label={`Copy ${label}`}>
          <Copy />
        </Button>
      ) : null}
    </div>
  );
}

function NewServerWizard({
  open,
  busy,
  loading,
  options,
  onClose,
  onRetry,
  onCreate,
}: {
  open: boolean;
  busy: boolean;
  loading: boolean;
  options: CreateInstanceOptions | null;
  onClose: () => void;
  onRetry: () => void;
  onCreate: (payload: {
    name: string;
    minecraft: string;
    size: InstanceSize;
    accept_eula: boolean;
  }) => Promise<void>;
}) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [name, setName] = useState("");
  const [minecraft, setMinecraft] = useState("");
  const [size, setSize] = useState<InstanceSize>("friends");
  const [eula, setEula] = useState(false);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose, open]);

  if (!open) return null;

  const recommendedVersion = options?.minecraft_versions.find((item) => item.recommended);
  const effectiveMinecraft =
    minecraft || recommendedVersion?.value || options?.minecraft_versions[0]?.value || "";
  const selectedSize = options?.sizes.find((item) => item.value === size);
  const next = () => {
    if (step === 1 && name.trim()) setStep(2);
    if (step === 2 && effectiveMinecraft) setStep(3);
  };
  const back = () => setStep((current) => (current === 3 ? 2 : 1));
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (step < 3) {
      next();
      return;
    }
    if (!eula || !name.trim() || !effectiveMinecraft) return;
    await onCreate({ name: name.trim(), minecraft: effectiveMinecraft, size, accept_eula: true });
  };

  return (
    <div
      className="fixed inset-0 z-[80] grid place-items-center overflow-y-auto bg-black/75 p-4 backdrop-blur-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="new-server-title"
    >
      <button className="absolute inset-0" onClick={() => !busy && onClose()} aria-label="Close dialog" />
      <form
        onSubmit={submit}
        className="relative my-6 w-full max-w-xl overflow-hidden rounded-[22px] border border-[var(--line-strong)] bg-[var(--panel)] shadow-[0_40px_120px_rgba(0,0,0,.62)]"
      >
        <div className="flex items-start justify-between border-b border-[var(--line)] px-5 py-5 sm:px-6">
          <div>
            <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-[var(--signal)]">
              Step {step} of 3
            </p>
            <h2 id="new-server-title" className="mt-2 text-xl font-semibold tracking-[-0.035em]">
              {step === 1
                ? "Name your server"
                : step === 2
                  ? "Choose Minecraft"
                  : "Choose a server size"}
            </h2>
          </div>
          <Button type="button" variant="ghost" size="icon-sm" onClick={onClose} disabled={busy} aria-label="Close dialog">
            <X />
          </Button>
        </div>

        <div className="grid grid-cols-3 gap-2 px-5 pt-5 sm:px-6" aria-hidden="true">
          {[1, 2, 3].map((value) => (
            <span
              key={value}
              className={cn(
                "h-1 rounded-full",
                value <= step ? "bg-[var(--signal)]" : "bg-[var(--panel-raised)]",
              )}
            />
          ))}
        </div>

        <div className="min-h-[330px] p-5 sm:p-6">
          {step === 1 ? (
            <div>
              <label htmlFor="server-name" className="text-sm font-semibold">
                What should we call it?
              </label>
              <Input
                id="server-name"
                autoFocus
                required
                maxLength={64}
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="Friends Survival"
                className="mt-3 h-12 text-base"
              />
              <div className="mt-4 flex items-start gap-3 rounded-xl border border-[var(--line)] bg-[var(--panel-deep)] p-4">
                <Check className="mt-0.5 size-4 shrink-0 text-[var(--signal)]" />
                <p className="text-xs leading-5 text-[var(--ink-muted)]">
                  That is all we need. Dart creates the technical ID and server folder automatically.
                </p>
              </div>
            </div>
          ) : null}

          {step === 2 ? (
            loading ? (
              <div className="grid min-h-64 place-items-center text-sm text-[var(--ink-muted)]">
                <div className="text-center">
                  <Loader2 className="mx-auto mb-3 size-5 animate-spin text-[var(--signal)]" />
                  Loading stable releases…
                </div>
              </div>
            ) : options && options.minecraft_versions.length > 0 ? (
              <div>
                {recommendedVersion ? (
                  <button
                    type="button"
                    onClick={() => setMinecraft(recommendedVersion.value)}
                    className={cn(
                      "flex w-full items-center gap-4 rounded-2xl border p-4 text-left transition",
                      effectiveMinecraft === recommendedVersion.value
                        ? "border-[var(--signal)]/40 bg-[var(--signal)]/[0.065]"
                        : "border-[var(--line)] bg-[var(--panel-deep)] hover:border-[var(--line-strong)]",
                    )}
                  >
                    <span className="grid size-10 place-items-center rounded-xl bg-[var(--signal)] text-[var(--signal-ink)]">
                      <Check className="size-4" />
                    </span>
                    <span className="flex-1">
                      <span className="flex items-center gap-2">
                        <span className="text-sm font-semibold">{recommendedVersion.label}</span>
                        <Badge tone="signal">Recommended</Badge>
                      </span>
                      <span className="mt-1 block text-xs text-[var(--ink-muted)]">Best choice for a new server</span>
                    </span>
                  </button>
                ) : null}
                <label className="mt-6 block">
                  <span className="text-xs font-semibold">Or choose another stable release</span>
                  <Select
                    value={effectiveMinecraft}
                    onChange={(event) => setMinecraft(event.target.value)}
                    className="mt-2 w-full"
                  >
                    {options.minecraft_versions.map((version) => (
                      <option key={version.value} value={version.value}>
                        {version.label}{version.recommended ? " · Recommended" : ""}
                      </option>
                    ))}
                  </Select>
                </label>
                <p className="mt-4 text-xs leading-5 text-[var(--ink-faint)]">
                  Dart picks compatible Fabric components automatically.
                </p>
              </div>
            ) : (
              <EmptyState
                icon={WifiOff}
                title="Could not load Minecraft releases"
                detail="Check the daemon connection and try again."
                action={
                  <Button type="button" variant="secondary" onClick={onRetry}>
                    <RefreshCw /> Try again
                  </Button>
                }
              />
            )
          ) : null}

          {step === 3 ? (
            <div>
              <div className="grid gap-3 sm:grid-cols-3">
                {options?.sizes.map((option) => {
                  const Icon =
                    option.value === "personal"
                      ? UserRound
                      : option.value === "friends"
                        ? Users
                        : Globe2;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setSize(option.value)}
                      className={cn(
                        "rounded-2xl border p-4 text-left transition",
                        size === option.value
                          ? "border-[var(--signal)]/40 bg-[var(--signal)]/[0.065]"
                          : "border-[var(--line)] bg-[var(--panel-deep)] hover:border-[var(--line-strong)]",
                      )}
                    >
                      <span className="flex items-center justify-between">
                        <Icon className={cn("size-5", size === option.value ? "text-[var(--signal)]" : "text-[var(--ink-muted)]")} />
                        {option.recommended ? <Badge tone="signal">Popular</Badge> : null}
                      </span>
                      <span className="mt-5 block text-sm font-semibold">{option.label}</span>
                      <span className="mt-1 block text-[11px] leading-4 text-[var(--ink-muted)]">{option.description}</span>
                      <span className="mt-3 block text-[10px] text-[var(--ink-faint)]">{formatMemory(option.memory_mib)} memory</span>
                    </button>
                  );
                })}
              </div>

              <label className="mt-5 flex cursor-pointer items-start gap-3 rounded-xl border border-[var(--line)] bg-[var(--panel-raised)] p-4">
                <input
                  type="checkbox"
                  required
                  checked={eula}
                  onChange={(event) => setEula(event.target.checked)}
                  className="mt-0.5 size-4 accent-[var(--signal)]"
                />
                <span>
                  <span className="block text-xs font-semibold">I accept the Minecraft EULA</span>
                  <span className="mt-1 block text-[11px] leading-4 text-[var(--ink-faint)]">
                    Required to create and run a Minecraft server.
                  </span>
                </span>
              </label>

              <p className="mt-4 text-center text-xs text-[var(--ink-faint)]">
                {name} · Minecraft {effectiveMinecraft} · {selectedSize?.label ?? "Friends"}
              </p>
            </div>
          ) : null}
        </div>

        <div className="flex items-center justify-between border-t border-[var(--line)] bg-[var(--panel-deep)] px-5 py-4 sm:px-6">
          <Button
            type="button"
            variant="ghost"
            onClick={step === 1 ? onClose : back}
            disabled={busy}
          >
            {step === 1 ? "Cancel" : "Back"}
          </Button>
          {step < 3 ? (
            <Button type="submit" disabled={(step === 1 && !name.trim()) || (step === 2 && !effectiveMinecraft) || loading}>
              Continue <ArrowRight />
            </Button>
          ) : (
            <Button type="submit" disabled={busy || !eula || !name.trim() || !effectiveMinecraft}>
              {busy ? <Loader2 className="animate-spin" /> : <Plus />}
              {busy ? "Creating server…" : "Create server"}
            </Button>
          )}
        </div>
      </form>
    </div>
  );
}
