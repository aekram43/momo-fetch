"use client";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { getMcpServers } from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";

/**
 * F14 — MCP server status.
 *
 * **`tool_count: null` renders as "—", never "0"** (spec G4). stdio servers all
 * share one merged toolset with a single `"mcp"` prefix, so there is no
 * per-server attribution; printing 0 would be indistinguishable from "connected
 * but exposes nothing", which is a different and much worse fact.
 */
export function ToolStatus() {
  const { data, error } = useGatewayResource(getMcpServers);

  return (
    <PanelSection
      title="Tools"
      action={
        data && (
          <span className="font-mono text-[10px] font-normal text-faint">
            {data.running}/{data.servers.length}
          </span>
        )
      }
    >
      {error ? (
        <Hint>Could not load MCP status.</Hint>
      ) : !data ? (
        <Hint>Loading…</Hint>
      ) : data.servers.length === 0 ? (
        <Hint>
          No MCP servers configured. See{" "}
          <code className="font-mono text-[11px] text-dim">.harness/mcp.json</code>.
        </Hint>
      ) : (
        <ul className="space-y-1">
          {data.servers.map((s) => (
            <li key={s.id} className="flex items-center gap-1.5" title={s.error ?? undefined}>
              <Dot tone={s.status === "running" ? "ok" : s.error ? "bad" : "off"} />
              <span className="truncate font-mono text-[11px] text-dim">{s.id}</span>
              <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
                {/* Null means unknown. Never print 0. */}
                {s.tool_count ?? "—"}
              </span>
            </li>
          ))}
        </ul>
      )}
    </PanelSection>
  );
}
