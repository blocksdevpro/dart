'use client';

import { useState } from 'react';
import { Terminal, Copy, Check, Sparkles } from 'lucide-react';

export function CommandBuilder() {
  const [instanceId, setInstanceId] = useState('survival-121');
  const [displayName, setDisplayName] = useState('Survival 1.21.4');
  const [mcVersion, setMcVersion] = useState('1.21.4');
  const [acceptEula, setAcceptEula] = useState(true);
  const [customHome, setCustomHome] = useState('');
  const [copied, setCopied] = useState(false);

  // Generate command string
  const homePart = customHome.trim() ? `--home "${customHome.trim()}" ` : '';
  const eulaPart = acceptEula ? ' --accept-eula' : '';
  const mcPart = mcVersion ? ` --minecraft ${mcVersion}` : '';
  const command = `dart ${homePart}create ${instanceId.trim() || 'my-server'} "${displayName.trim() || 'My Server'}"${mcPart}${eulaPart}`;

  const copyToClipboard = () => {
    navigator.clipboard.writeText(command);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="not-prose my-6 rounded-xl border border-fd-border bg-fd-card/50 p-5 shadow-sm backdrop-blur-sm">
      <div className="flex items-center justify-between gap-2 border-b border-fd-border pb-3 mb-4">
        <div className="flex items-center gap-2">
          <Sparkles className="size-4 text-emerald-500" />
          <h3 className="font-semibold text-sm text-fd-foreground">Interactive Command Generator</h3>
        </div>
        <span className="text-xs text-fd-muted-foreground bg-fd-muted px-2 py-0.5 rounded-full font-mono">
          CLI Generator
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-sm mb-4">
        <div>
          <label className="block text-xs font-medium text-fd-muted-foreground mb-1">
            Instance ID <span className="text-xs text-fd-muted-foreground/70">(slug)</span>
          </label>
          <input
            type="text"
            value={instanceId}
            onChange={(e) => setInstanceId(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
            placeholder="e.g. survival-121"
            className="w-full rounded-md border border-fd-input bg-fd-background px-3 py-1.5 text-xs text-fd-foreground focus:outline-none focus:ring-1 focus:ring-emerald-500 font-mono"
          />
        </div>

        <div>
          <label className="block text-xs font-medium text-fd-muted-foreground mb-1">
            Display Name
          </label>
          <input
            type="text"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder="e.g. Survival 1.21.4"
            className="w-full rounded-md border border-fd-input bg-fd-background px-3 py-1.5 text-xs text-fd-foreground focus:outline-none focus:ring-1 focus:ring-emerald-500"
          />
        </div>

        <div>
          <label className="block text-xs font-medium text-fd-muted-foreground mb-1">
            Minecraft Version
          </label>
          <select
            value={mcVersion}
            onChange={(e) => setMcVersion(e.target.value)}
            className="w-full rounded-md border border-fd-input bg-fd-background px-3 py-1.5 text-xs text-fd-foreground focus:outline-none focus:ring-1 focus:ring-emerald-500 font-mono"
          >
            <option value="1.21.4">1.21.4 (Latest)</option>
            <option value="1.21.1">1.21.1</option>
            <option value="1.20.4">1.20.4</option>
            <option value="1.20.1">1.20.1</option>
            <option value="1.19.4">1.19.4</option>
          </select>
        </div>

        <div className="flex flex-col justify-end">
          <label className="flex items-center gap-2 cursor-pointer py-1.5">
            <input
              type="checkbox"
              checked={acceptEula}
              onChange={(e) => setAcceptEula(e.target.checked)}
              className="rounded border-fd-input text-emerald-600 focus:ring-emerald-500 size-4"
            />
            <span className="text-xs font-medium text-fd-foreground">
              Accept Minecraft EULA (<code className="text-xs font-mono">--accept-eula</code>)
            </span>
          </label>
        </div>
      </div>

      <div className="relative rounded-lg bg-black/90 p-3 text-emerald-400 font-mono text-xs border border-emerald-950/60 shadow-inner flex items-center justify-between gap-2 overflow-x-auto">
        <div className="flex items-center gap-2 flex-1 min-w-0">
          <Terminal className="size-4 shrink-0 text-emerald-500" />
          <span className="truncate select-all text-emerald-300">{command}</span>
        </div>
        <button
          type="button"
          onClick={copyToClipboard}
          className="shrink-0 flex items-center gap-1.5 rounded bg-emerald-900/40 hover:bg-emerald-800/60 text-emerald-300 border border-emerald-700/50 px-2.5 py-1 text-xs transition"
          title="Copy command"
        >
          {copied ? (
            <>
              <Check className="size-3.5 text-emerald-400" />
              <span>Copied!</span>
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
  );
}
