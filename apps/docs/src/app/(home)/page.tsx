'use client';

import Link from 'next/link';
import { useState } from 'react';
import { 
  Terminal, 
  Cpu, 
  Box, 
  ArrowRight, 
  Check, 
  Copy, 
  Zap, 
  ShieldCheck, 
  FolderTree, 
  Sparkles 
} from 'lucide-react';
import { TuiPreview } from '@/components/TuiPreview';

export default function HomePage() {
  const [copied, setCopied] = useState(false);
  const installCmd = 'cargo install --git https://github.com/blocksdevpro/dart.git';

  const copyInstall = () => {
    navigator.clipboard.writeText(installCmd);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="flex flex-col min-h-screen">
      {/* Hero Section */}
      <section className="relative overflow-hidden pt-16 pb-12 md:pt-24 md:pb-20 border-b border-fd-border/50">
        <div className="absolute inset-0 bg-radial from-emerald-500/10 via-transparent to-transparent pointer-events-none" />
        
        <div className="max-w-5xl mx-auto px-4 sm:px-6 text-center relative z-10">
          <div className="inline-flex items-center gap-2 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-3 py-1 text-xs font-medium text-emerald-400 mb-6">
            <Sparkles className="size-3.5" />
            <span>Built with Rust, Ratatui & Tokio</span>
          </div>

          <h1 className="text-4xl sm:text-6xl font-extrabold tracking-tight text-fd-foreground mb-6">
            Modern Fabric Server Manager <br />
            <span className="text-transparent bg-clip-text bg-gradient-to-r from-emerald-400 via-teal-300 to-cyan-400">
              For CLI & Terminal UIs
            </span>
          </h1>

          <p className="text-lg sm:text-xl text-fd-muted-foreground max-w-2xl mx-auto mb-8 font-normal leading-relaxed">
            Dart is a high-performance Minecraft Fabric instance manager. Bootstrap servers in seconds, monitor live console streams in a Ratatui TUI, and manage Modrinth add-ons effortlessly.
          </p>

          {/* Action Buttons & Install Bar */}
          <div className="flex flex-col sm:flex-row items-center justify-center gap-4 mb-10">
            <Link
              href="/docs/quickstart"
              className="w-full sm:w-auto inline-flex items-center justify-center gap-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 px-6 py-3 font-semibold text-white transition shadow-lg shadow-emerald-900/20"
            >
              Get Started <ArrowRight className="size-4" />
            </Link>

            <Link
              href="/docs"
              className="w-full sm:w-auto inline-flex items-center justify-center gap-2 rounded-lg border border-fd-border bg-fd-secondary/60 hover:bg-fd-secondary px-6 py-3 font-medium text-fd-foreground transition"
            >
              Documentation
            </Link>
          </div>

          {/* Copyable Quick Install */}
          <div className="inline-flex items-center gap-3 rounded-xl border border-fd-border bg-fd-card/80 p-2 text-xs font-mono text-fd-muted-foreground shadow-sm max-w-xl mx-auto backdrop-blur-md">
            <Terminal className="size-4 text-emerald-500 ml-2 shrink-0" />
            <span className="truncate select-all text-fd-foreground">{installCmd}</span>
            <button
              onClick={copyInstall}
              type="button"
              className="flex items-center gap-1.5 rounded-md bg-fd-muted hover:bg-fd-muted/80 px-2.5 py-1 text-xs font-sans text-fd-foreground transition shrink-0"
            >
              {copied ? (
                <>
                  <Check className="size-3.5 text-emerald-400" />
                  <span>Copied</span>
                </>
              ) : (
                <>
                  <Copy className="size-3.5" />
                  <span>Copy</span>
                </>
              )}
            </button>
          </div>
        </div>
      </section>

      {/* Interactive TUI Showcase */}
      <section className="py-12 md:py-16 max-w-5xl mx-auto px-4 sm:px-6 w-full">
        <div className="text-center mb-6">
          <h2 className="text-2xl font-bold text-fd-foreground">Experience Dart in Your Terminal</h2>
          <p className="text-sm text-fd-muted-foreground mt-1">
            Keyboard-driven navigation, real-time log tailing, and instant process controls.
          </p>
        </div>

        <TuiPreview />
      </section>

      {/* Feature Grid */}
      <section className="py-16 border-t border-fd-border/50 bg-fd-muted/20">
        <div className="max-w-5xl mx-auto px-4 sm:px-6">
          <div className="text-center max-w-2xl mx-auto mb-12">
            <h2 className="text-2xl sm:text-3xl font-bold text-fd-foreground">
              Engineered for Speed, Reliability, and Simplicity
            </h2>
            <p className="text-sm text-fd-muted-foreground mt-2">
              Everything you need to orchestrate local development servers, private SMPs, or automated benchmarks.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-emerald-500/10 flex items-center justify-center text-emerald-400 mb-4">
                <Zap className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Automated Fabric Meta</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Connects directly to the Fabric Meta API to resolve loader and installer combinations. Caches reusable launcher JARs locally for instant, offline startups.
              </p>
            </div>

            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-cyan-500/10 flex items-center justify-center text-cyan-400 mb-4">
                <Cpu className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Tokio Process Supervisor</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Manages Java child processes asynchronously with non-blocking I/O multiplexing. Stream console stdout/stderr and execute graceful shutdowns safely.
              </p>
            </div>

            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-purple-500/10 flex items-center justify-center text-purple-400 mb-4">
                <ShieldCheck className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Modrinth Content Engine</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Install Fabric mods, modpacks (.mrpack), and data packs. Every file is strictly validated against cryptographic SHA-1 and SHA-512 hashes.
              </p>
            </div>

            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-amber-500/10 flex items-center justify-center text-amber-400 mb-4">
                <Terminal className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Ergonomic CLI</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Typed command-line parsing verifies arguments before touching disks or networks. Scriptable commands for headless server automation and CI.
              </p>
            </div>

            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-blue-500/10 flex items-center justify-center text-blue-400 mb-4">
                <FolderTree className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Isolated Storage Layout</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Clean XDG-compliant directory structure. Each server instance is completely isolated with its own world, config, logs, and mods.
              </p>
            </div>

            <div className="rounded-xl border border-fd-border bg-fd-card p-6 shadow-sm">
              <div className="size-10 rounded-lg bg-pink-500/10 flex items-center justify-center text-pink-400 mb-4">
                <Box className="size-5" />
              </div>
              <h3 className="text-base font-semibold text-fd-foreground mb-2">Ratatui Dashboard</h3>
              <p className="text-sm text-fd-muted-foreground leading-relaxed">
                Full-screen terminal interface with live log streaming, mod browser, process PID monitoring, and keyboard shortcuts.
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="mt-auto py-8 border-t border-fd-border text-center text-xs text-fd-muted-foreground">
        <p>Dart • Local Fabric server instance manager CLI & TUI in Rust.</p>
        <p className="mt-1">
          Open-source under MIT License. Documentation powered by Fumadocs & Next.js.
        </p>
      </footer>
    </div>
  );
}
