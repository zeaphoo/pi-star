"use client";

import { useLayoutEffect, useRef } from "react";
import { openStackedDialog } from "@/lib/stacked-dialog";
import { SubagentViewer } from "./SubagentViewer";

interface Props {
  sessionId: string;
  label: string;
  running: boolean;
  onClose: () => void;
  onOpenFile: (filePath: string, page?: number) => void;
  /** Opens a nested sub-agent's transcript in this dialog. */
  onOpenSubagent?: (sessionId: string, label: string) => void;
}

/**
 * A sub-agent's read-only transcript, read in a modal of its own instead of a
 * tab beside the files: the artifacts pane stays about artifacts. Escape, the
 * backdrop, and the viewer's own close button all close it; focus returns to
 * whatever opened it.
 */
export function SubagentDialog({ sessionId, label, running, onClose, onOpenFile, onOpenSubagent }: Props) {
  const panelRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (typeof document === "undefined") return;
    return openStackedDialog(document, panelRef.current, onClose);
  }, [onClose]);

  return (
    <div
      className="subagent-dialog-backdrop"
      data-subagent-dialog-backdrop=""
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        className="subagent-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
      >
        <SubagentViewer
          key={sessionId}
          sessionId={sessionId}
          fallbackLabel={label}
          running={running}
          onOpenFile={onOpenFile}
          onOpenSubagent={onOpenSubagent}
          onClose={onClose}
        />
      </div>
    </div>
  );
}
