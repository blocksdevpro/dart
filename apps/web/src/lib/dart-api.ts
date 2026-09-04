export type InstanceStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "failed";

export type InstanceState =
  | { status: "stopped" | "starting" | "stopping" }
  | { status: "running"; pid: number }
  | { status: "failed"; message: string };

export type FabricRuntime = {
  minecraft: string;
  loader: string;
  installer: string;
};

export type DartInstance = {
  id: string;
  name: string;
  root: string;
  config: {
    format_version: number;
    name: string;
    java: string;
    min_memory_mib: number;
    max_memory_mib: number;
    fabric: FabricRuntime;
  };
  state: InstanceState;
};

export type HealthResponse = {
  status: string;
  version: string;
  uptime_seconds: number;
};

export type SystemInfo = {
  version: string;
  pid: number;
  dart_home: string;
  socket_path: string;
  instances_count: number;
  runtimes_count: number;
};

export type InstanceSize = "personal" | "friends" | "community";

export type CreateInstanceOptions = {
  minecraft_versions: Array<{
    value: string;
    label: string;
    recommended: boolean;
  }>;
  sizes: Array<{
    value: InstanceSize;
    label: string;
    description: string;
    memory_mib: number;
    recommended: boolean;
  }>;
};

export type ContentKind = "mod" | "data_pack" | "resource_pack";

export type InstalledContent = {
  kind: ContentKind;
  key: string;
  file_name: string;
  name: string;
  version?: string;
  managed: boolean;
  project_id?: string;
};

export type ContentSearchHit = {
  id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
  icon_url?: string;
  kind: ContentKind;
};

export type ContentInstallReport = {
  kind: ContentKind;
  title: string;
  version: string;
  outcome: "added" | "updated" | "already_installed";
  dependencies: string[];
};

export type ConsoleLine = {
  id: string;
  stream: "stdout" | "stderr";
  line: string;
  timestamp_millis: number;
};

type ApiErrorBody = {
  code?: string;
  message?: string;
};

export class DartApiError extends Error {
  status: number;
  code?: string;

  constructor(message: string, status: number, code?: string) {
    super(message);
    this.name = "DartApiError";
    this.status = status;
    this.code = code;
  }
}

const cleanBase = (baseUrl: string) => baseUrl.replace(/\/$/, "");

function invalidResponse(detail: string): never {
  throw new DartApiError(`The daemon returned an invalid response: ${detail}`, 502);
}

function objectValue(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return invalidResponse(`${name} is not an object`);
  }
  return value as Record<string, unknown>;
}

function stringValue(value: unknown, name: string): string {
  if (typeof value !== "string") return invalidResponse(`${name} is not text`);
  return value;
}

function numberValue(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return invalidResponse(`${name} is not a number`);
  }
  return value;
}

function booleanValue(value: unknown, name: string): boolean {
  if (typeof value !== "boolean") return invalidResponse(`${name} is not true or false`);
  return value;
}

function arrayValue(value: unknown, name: string): unknown[] {
  if (!Array.isArray(value)) return invalidResponse(`${name} is not a list`);
  return value;
}

function optionalString(value: unknown, name: string): string | undefined {
  return value === undefined || value === null ? undefined : stringValue(value, name);
}

function oneOf<const T extends readonly string[]>(
  value: unknown,
  options: T,
  name: string,
): T[number] {
  const parsed = stringValue(value, name);
  if (!options.includes(parsed)) return invalidResponse(`${name} is unknown`);
  return parsed as T[number];
}

function parseRuntime(value: unknown): FabricRuntime {
  const item = objectValue(value, "runtime");
  return {
    minecraft: stringValue(item.minecraft, "runtime.minecraft"),
    loader: stringValue(item.loader, "runtime.loader"),
    installer: stringValue(item.installer, "runtime.installer"),
  };
}

function parseState(value: unknown): InstanceState {
  const state = objectValue(value, "instance.state");
  const status = oneOf(
    state.status,
    ["stopped", "starting", "running", "stopping", "failed"] as const,
    "instance.state.status",
  );
  switch (status) {
    case "running":
      return { status, pid: numberValue(state.pid, "instance.state.pid") };
    case "failed":
      return { status, message: stringValue(state.message, "instance.state.message") };
    case "stopped":
    case "starting":
    case "stopping":
      return { status };
  }
}

function parseInstance(value: unknown): DartInstance {
  const item = objectValue(value, "instance");
  const config = objectValue(item.config, "instance.config");
  return {
    id: stringValue(item.id, "instance.id"),
    name: stringValue(item.name, "instance.name"),
    root: stringValue(item.root, "instance.root"),
    config: {
      format_version: numberValue(config.format_version, "instance.config.format_version"),
      name: stringValue(config.name, "instance.config.name"),
      java: stringValue(config.java, "instance.config.java"),
      min_memory_mib: numberValue(
        config.min_memory_mib,
        "instance.config.min_memory_mib",
      ),
      max_memory_mib: numberValue(
        config.max_memory_mib,
        "instance.config.max_memory_mib",
      ),
      fabric: parseRuntime(config.fabric),
    },
    state: parseState(item.state),
  };
}

function parseHealth(value: unknown): HealthResponse {
  const item = objectValue(value, "health");
  return {
    status: stringValue(item.status, "health.status"),
    version: stringValue(item.version, "health.version"),
    uptime_seconds: numberValue(item.uptime_seconds, "health.uptime_seconds"),
  };
}

function parseSystem(value: unknown): SystemInfo {
  const item = objectValue(value, "system");
  return {
    version: stringValue(item.version, "system.version"),
    pid: numberValue(item.pid, "system.pid"),
    dart_home: stringValue(item.dart_home, "system.dart_home"),
    socket_path: stringValue(item.socket_path, "system.socket_path"),
    instances_count: numberValue(item.instances_count, "system.instances_count"),
    runtimes_count: numberValue(item.runtimes_count, "system.runtimes_count"),
  };
}

function parseCreationOptions(value: unknown): CreateInstanceOptions {
  const item = objectValue(value, "create options");
  return {
    minecraft_versions: arrayValue(item.minecraft_versions, "minecraft_versions").map(
      (entry) => {
        const version = objectValue(entry, "minecraft version");
        return {
          value: stringValue(version.value, "minecraft version.value"),
          label: stringValue(version.label, "minecraft version.label"),
          recommended: booleanValue(version.recommended, "minecraft version.recommended"),
        };
      },
    ),
    sizes: arrayValue(item.sizes, "sizes").map((entry) => {
      const size = objectValue(entry, "size");
      return {
        value: oneOf(
          size.value,
          ["personal", "friends", "community"] as const,
          "size.value",
        ),
        label: stringValue(size.label, "size.label"),
        description: stringValue(size.description, "size.description"),
        memory_mib: numberValue(size.memory_mib, "size.memory_mib"),
        recommended: booleanValue(size.recommended, "size.recommended"),
      };
    }),
  };
}

function parseContentKind(value: unknown, name: string): ContentKind {
  return oneOf(value, ["mod", "data_pack", "resource_pack"] as const, name);
}

function parseInstalledContent(value: unknown): InstalledContent {
  const item = objectValue(value, "installed add-on");
  return {
    kind: parseContentKind(item.kind, "installed add-on.kind"),
    key: stringValue(item.key, "installed add-on.key"),
    file_name: stringValue(item.file_name, "installed add-on.file_name"),
    name: stringValue(item.name, "installed add-on.name"),
    version: optionalString(item.version, "installed add-on.version"),
    managed: booleanValue(item.managed, "installed add-on.managed"),
    project_id: optionalString(item.project_id, "installed add-on.project_id"),
  };
}

function parseSearchHit(value: unknown): ContentSearchHit {
  const item = objectValue(value, "search result");
  return {
    id: stringValue(item.id, "search result.id"),
    slug: stringValue(item.slug, "search result.slug"),
    title: stringValue(item.title, "search result.title"),
    description: stringValue(item.description, "search result.description"),
    author: stringValue(item.author, "search result.author"),
    downloads: numberValue(item.downloads, "search result.downloads"),
    icon_url: optionalString(item.icon_url, "search result.icon_url"),
    kind: parseContentKind(item.kind, "search result.kind"),
  };
}

function parseInstallReport(value: unknown): ContentInstallReport {
  const item = objectValue(value, "install report");
  return {
    kind: parseContentKind(item.kind, "install report.kind"),
    title: stringValue(item.title, "install report.title"),
    version: stringValue(item.version, "install report.version"),
    outcome: oneOf(
      item.outcome,
      ["added", "updated", "already_installed"] as const,
      "install report.outcome",
    ),
    dependencies: arrayValue(item.dependencies, "install report.dependencies").map(
      (dependency) => stringValue(dependency, "install report.dependency"),
    ),
  };
}

function parseConsoleLine(value: unknown): ConsoleLine {
  const item = objectValue(value, "console line");
  return {
    id: stringValue(item.id, "console line.id"),
    stream: oneOf(item.stream, ["stdout", "stderr"] as const, "console line.stream"),
    line: stringValue(item.line, "console line.line"),
    timestamp_millis: numberValue(item.timestamp_millis, "console line.timestamp_millis"),
  };
}

export class DartApi {
  private baseUrl: string;

  constructor(baseUrl = "/dart-api") {
    this.baseUrl = cleanBase(baseUrl);
  }

  private async request(path: string, init?: RequestInit): Promise<unknown> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      cache: "no-store",
      headers: {
        ...(init?.body ? { "Content-Type": "application/json" } : {}),
        ...init?.headers,
      },
    });

    if (!response.ok) {
      let body: ApiErrorBody | undefined;
      try {
        const parsed: unknown = await response.json();
        const envelope = objectValue(parsed, "error response");
        const record = objectValue(envelope.error, "error");
        body = {
          code: optionalString(record.code, "error.code"),
          message: optionalString(record.message, "error.message"),
        };
      } catch {
        body = undefined;
      }
      throw new DartApiError(
        body?.message || `Daemon request failed (${response.status})`,
        response.status,
        body?.code,
      );
    }

    if (response.status === 204 || response.headers.get("content-length") === "0") {
      return undefined;
    }

    const text = await response.text();
    if (!text) return undefined;
    try {
      return JSON.parse(text) as unknown;
    } catch {
      return invalidResponse("response body is not JSON");
    }
  }

  async health() {
    return parseHealth(await this.request("/health"));
  }

  async system() {
    return parseSystem(await this.request("/system"));
  }

  async listInstances() {
    return arrayValue(await this.request("/instances"), "instances").map(parseInstance);
  }

  async creationOptions() {
    return parseCreationOptions(await this.request("/instances/options"));
  }

  async createInstance(payload: {
    name: string;
    minecraft: string;
    size: InstanceSize;
    accept_eula: boolean;
  }) {
    return parseInstance(
      await this.request("/instances", {
        method: "POST",
        body: JSON.stringify(payload),
      }),
    );
  }

  async instanceAction(id: string, action: "start" | "stop" | "restart" | "kill") {
    await this.request(`/instances/${encodeURIComponent(id)}/${action}`, { method: "POST" });
  }

  async listContent(id: string, kind: ContentKind) {
    const value = await this.request(
      `/instances/${encodeURIComponent(id)}/content?kind=${kind}`,
    );
    return arrayValue(value, "installed add-ons").map(parseInstalledContent);
  }

  async searchContent(id: string, query: string, kind: ContentKind) {
    const params = new URLSearchParams({ query, kind });
    const value = await this.request(
      `/instances/${encodeURIComponent(id)}/content/search?${params}`,
    );
    return arrayValue(value, "search results").map(parseSearchHit);
  }

  async installContent(id: string, projectId: string, kind: ContentKind) {
    return parseInstallReport(
      await this.request(`/instances/${encodeURIComponent(id)}/content/install`, {
        method: "POST",
        body: JSON.stringify({ project_id: projectId, kind }),
      }),
    );
  }

  async removeContent(id: string, key: string, kind: ContentKind) {
    await this.request(`/instances/${encodeURIComponent(id)}/content/remove`, {
      method: "POST",
      body: JSON.stringify({ key, kind }),
    });
  }

  async logs(id: string, tail = 80) {
    const value = await this.request(
      `/instances/${encodeURIComponent(id)}/logs?tail=${tail}`,
    );
    return arrayValue(value, "console lines").map(parseConsoleLine);
  }

  async command(id: string, command: string) {
    await this.request(`/instances/${encodeURIComponent(id)}/command`, {
      method: "POST",
      body: JSON.stringify({ command }),
    });
  }
}

export function readableError(error: unknown) {
  return error instanceof Error ? error.message : "Something went wrong";
}
