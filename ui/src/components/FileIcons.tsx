"use client";

import type { CSSProperties } from "react";
import { useFileIconTheme } from "@/hooks/useFileIconTheme";
import {
  resolveFileIcon,
  resolveFolderIcon,
  resolveGenericFileIcon,
  type IconThemeId,
  type ResolvedIcon,
} from "@/lib/icon-themes";

interface IconProps {
  size?: number;
  /** Show this theme instead of the tree's setting, for a Settings preview. */
  theme?: IconThemeId;
}

function IconSpan({ icon, size }: { icon: ResolvedIcon; size: number }) {
  const style = {
    width: size,
    height: size,
    "--file-icon-light": `url(${icon.light})`,
    "--file-icon-dark": `url(${icon.dark})`,
  } as CSSProperties;

  return <span aria-hidden="true" className="file-icon" style={style} />;
}

export function FolderIcon({ size = 14, open = false, name, theme }: IconProps & { open?: boolean; name?: string }) {
  const { theme: activeTheme } = useFileIconTheme();
  return <IconSpan icon={resolveFolderIcon(theme ?? activeTheme, name, open)} size={size} />;
}

export function FileIcon({ name, size = 14, theme }: IconProps & { name: string }) {
  const { theme: activeTheme } = useFileIconTheme();
  return <IconSpan icon={resolveFileIcon(theme ?? activeTheme, name)} size={size} />;
}

export function GenericFileIcon({ size = 14, theme }: IconProps) {
  const { theme: activeTheme } = useFileIconTheme();
  return <IconSpan icon={resolveGenericFileIcon(theme ?? activeTheme)} size={size} />;
}