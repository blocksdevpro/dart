'use client';

import { useState, useEffect } from 'react';
import { Play, Square, RefreshCw, Terminal, Box, ShieldCheck, Cpu } from 'lucide-react';

interface InstanceData {
  id: string;
  name: string;
  mc: string;
  loader: string;
  status: 'running' | 'stopped';
  pid?: number;
  memory: string;
  modsCount: number;
}

export function TuiPreview() {
  const [instances, setInstances] = useState<InstanceData[]>([
    {
      id: 'survival-121',
      name: 'Survival SMP',
      mc: '1.21.4',
      loader: '0.16.10',
      status: 'running',
      pid: 41829,
      memory: '4096M',
      modsCount: 14,
    },
    {
      id: 'creative-lab',
      name: 'Redstone Lab',
      mc: '1.21.1',
      loader: '0.16.9',
      status: 'stopped',
      memory: '2048M',
      modsCount: 6,
    },
    {
      id: 'hardcore-s2',
      name: 'Hardcore World',
      mc: '1.20.4',
      loader: '0.15.11',
      status: 'stopped',
      memory: '4096M',
      modsCount: 22,
    },
  ]);

  const [selectedIdx, setSelectedIdx] = useState(0);
  const [activeTab, setActiveTab] = useState<'console' | 'mods'>('console');
  const [logs, setLogs] = useState<string[]>([
    '[05:40:12 INFO] Loading Minecraft 1.21.4 with Fabric Loader 0.16.10',
    '[05:40:13 INFO] [FabricLoader] Loading 14 mods: fabric-api 0.110.0, lithium 0.14.7, ferritcore 7.0.0...',
    '[05:40:14 INFO] Preparing level "world"',
    '[05:40:15 INFO] Preparing start region for dimension minecraft:overworld',
    '[05:40:17 INFO] Done (4.812s)! For help, type "help"',
    '[05:41:02 INFO] [Server] Player Alex joined the game (127.0.0.1:54912)',
    '[05:41:30 INFO] <Alex> Hello from Fabric on Dart!',
  ]);

  const active = instances[selectedIdx];

  const toggleServer = (idx: number) => {
    setInstances((prev) =>
      prev.map((inst, i) => {
        if (i !== idx) return inst;
        if (inst.status === 'running') {
          return { ...inst, status: 'stopped', pid: undefined };
        } else {
          return { ...inst, status: 'running', pid: Math.floor(10000 + Math.random() * 80000) };
        }
      })
    );
  };

  return (
    <div className="not-prose my-8 overflow-hidden rounded-xl border border-zinc-800 bg-[#0c0e14] shadow-2xl font-mono text-xs text-zinc-300">
      {/* Window Title Bar */}
      <div className="flex items-center justify-between border-b border-zinc-800/80 bg-zinc-950/80 px-4 py-2 text-zinc-400">
        <div className="flex items-center gap-2">
          <span className="size-2.5 rounded-full bg-red-500/80 inline-block" />
          <span className="size-2.5 rounded-full bg-yellow-500/80 inline-block" />
          <span className="size-2.5 rounded-full bg-green-500/80 inline-block" />
          <span className="ml-2 font-semibold text-zinc-200">dart — Ratatui TUI Interface</span>
        </div>
        <div className="flex items-center gap-3 text-[11px]">
          <span className="text-zinc-500">v0.1.0</span>
          <span className="rounded bg-emerald-950/60 px-2 py-0.5 text-emerald-400 border border-emerald-800/50">
            daemon: connected
          </span>
        </div>
      </div>

      {/* Main Terminal Body */}
      <div className="grid grid-cols-1 md:grid-cols-12 min-h-[380px]">
        {/* Left Side: Instance List Pane */}
        <div className="md:col-span-4 border-r border-zinc-800/80 bg-zinc-950/40 p-3 flex flex-col justify-between">
          <div>
            <div className="flex items-center justify-between pb-2 mb-2 border-b border-zinc-800/60 text-zinc-400 uppercase tracking-wider text-[10px]">
              <span>Instances ({instances.length})</span>
              <span>State</span>
            </div>

            <div className="space-y-1">
              {instances.map((inst, i) => {
                const isSelected = i === selectedIdx;
                return (
                  <button
                    key={inst.id}
                    onClick={() => setSelectedIdx(i)}
                    className={`w-full text-left rounded px-2.5 py-2 transition flex items-center justify-between ${
                      isSelected
                        ? 'bg-emerald-950/70 border border-emerald-500/40 text-emerald-200 shadow-sm'
                        : 'hover:bg-zinc-900/60 text-zinc-400 border border-transparent'
                    }`}
                  >
                    <div>
                      <div className="font-semibold text-[11px] text-zinc-200 flex items-center gap-1.5">
                        <span>{inst.name}</span>
                      </div>
                      <div className="text-[10px] text-zinc-500">
                        {inst.mc} • {inst.loader}
                      </div>
                    </div>

                    <span
                      className={`px-1.5 py-0.5 rounded text-[10px] font-bold ${
                        inst.status === 'running'
                          ? 'bg-emerald-500/20 text-emerald-400'
                          : 'bg-zinc-800 text-zinc-400'
                      }`}
                    >
                      {inst.status === 'running' ? 'RUNNING' : 'STOPPED'}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="mt-4 pt-3 border-t border-zinc-800/60 text-[10px] text-zinc-500">
            <div>DART_HOME: ~/.local/share/dart</div>
            <div className="text-zinc-600">Storage: isolated instance dirs</div>
          </div>
        </div>

        {/* Right Side: Selected Instance Dashboard */}
        <div className="md:col-span-8 p-4 flex flex-col justify-between bg-zinc-900/20">
          <div>
            {/* Instance Header & Controls */}
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-zinc-800/80 pb-3 mb-3">
              <div>
                <div className="flex items-center gap-2">
                  <h4 className="text-sm font-bold text-zinc-100">{active.name}</h4>
                  <span className="text-[10px] text-zinc-500">({active.id})</span>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-zinc-400 mt-0.5">
                  <span className="flex items-center gap-1">
                    <Box className="size-3 text-emerald-400" /> Fabric {active.mc}
                  </span>
                  <span className="flex items-center gap-1">
                    <Cpu className="size-3 text-cyan-400" /> {active.memory} RAM
                  </span>
                  <span className="flex items-center gap-1 text-emerald-400">
                    <ShieldCheck className="size-3" /> EULA Accepted
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <button
                  onClick={() => toggleServer(selectedIdx)}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded font-medium text-xs transition shadow-sm ${
                    active.status === 'running'
                      ? 'bg-red-950/70 hover:bg-red-900 text-red-300 border border-red-800/60'
                      : 'bg-emerald-600 hover:bg-emerald-500 text-black font-semibold'
                  }`}
                >
                  {active.status === 'running' ? (
                    <>
                      <Square className="size-3 fill-current" /> Stop
                    </>
                  ) : (
                    <>
                      <Play className="size-3 fill-current" /> Start
                    </>
                  )}
                </button>
              </div>
            </div>

            {/* Tab navigation */}
            <div className="flex items-center gap-2 mb-2">
              <button
                onClick={() => setActiveTab('console')}
                className={`px-2.5 py-1 rounded text-[11px] font-medium transition ${
                  activeTab === 'console'
                    ? 'bg-zinc-800 text-zinc-100 border border-zinc-700'
                    : 'text-zinc-500 hover:text-zinc-300'
                }`}
              >
                Live Console ({active.status === 'running' ? `PID ${active.pid}` : 'Offline'})
              </button>
              <button
                onClick={() => setActiveTab('mods')}
                className={`px-2.5 py-1 rounded text-[11px] font-medium transition ${
                  activeTab === 'mods'
                    ? 'bg-zinc-800 text-zinc-100 border border-zinc-700'
                    : 'text-zinc-500 hover:text-zinc-300'
                }`}
              >
                Mods ({active.modsCount})
              </button>
            </div>

            {/* Viewport content */}
            {activeTab === 'console' ? (
              <div className="rounded border border-zinc-800/80 bg-black/60 p-3 h-[200px] overflow-y-auto space-y-1 font-mono text-[11px]">
                {active.status === 'running' ? (
                  logs.map((line, idx) => (
                    <div
                      key={idx}
                      className={
                        line.includes('WARN')
                          ? 'text-yellow-400'
                          : line.includes('Alex')
                          ? 'text-cyan-300'
                          : 'text-zinc-400'
                      }
                    >
                      {line}
                    </div>
                  ))
                ) : (
                  <div className="h-full flex flex-col items-center justify-center text-zinc-600 italic">
                    Server process stopped. Click "Start" or press [Enter] in the TUI to boot.
                  </div>
                )}
              </div>
            ) : (
              <div className="rounded border border-zinc-800/80 bg-black/60 p-3 h-[200px] overflow-y-auto font-mono text-[11px] space-y-1.5">
                <div className="text-zinc-400 pb-1 border-b border-zinc-800 flex justify-between">
                  <span>Mod / Modpack</span>
                  <span>Provider & Hash</span>
                </div>
                <div className="flex justify-between text-zinc-300">
                  <span>fabric-api-0.110.0+1.21.4.jar</span>
                  <span className="text-emerald-400 text-[10px]">Modrinth [SHA1: 8b7e...]</span>
                </div>
                <div className="flex justify-between text-zinc-300">
                  <span>lithium-fabric-mc1.21.4-0.14.7.jar</span>
                  <span className="text-emerald-400 text-[10px]">Modrinth [SHA1: c41a...]</span>
                </div>
                <div className="flex justify-between text-zinc-300">
                  <span>ferrite-core-7.0.0-fabric.jar</span>
                  <span className="text-emerald-400 text-[10px]">Modrinth [SHA1: 3f91...]</span>
                </div>
                <div className="text-zinc-500 pt-2 text-[10px]">
                  Managed via <code className="text-zinc-400">dart-daemon::content::ModManager</code>
                </div>
              </div>
            )}
          </div>

          {/* Bottom Interactive Command Bar */}
          <div className="mt-3 pt-2 border-t border-zinc-800/80 flex items-center justify-between text-[10px] text-zinc-500">
            <div className="flex items-center gap-2">
              <span className="bg-zinc-800 text-zinc-300 px-1 rounded">[Tab]</span> Switch Pane
              <span className="bg-zinc-800 text-zinc-300 px-1 rounded">[Enter]</span> Start/Stop
              <span className="bg-zinc-800 text-zinc-300 px-1 rounded">[m]</span> Mod Browser
              <span className="bg-zinc-800 text-zinc-300 px-1 rounded">[q]</span> Quit
            </div>
            <div className="text-emerald-500/80">Active Tokiosupervisor: Healthy</div>
          </div>
        </div>
      </div>
    </div>
  );
}
