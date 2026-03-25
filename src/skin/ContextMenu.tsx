/**
 * Shared skin-themed context menu — renders as a portal so it can
 * overflow any parent container. Styled using the active skin's
 * playlist colors.
 */

import { useEffect } from "react";
import { createPortal } from "react-dom";

export interface MenuItemDef {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}

export type MenuEntry = MenuItemDef | "separator";

interface ContextMenuProps {
  x: number;
  y: number;
  items: MenuEntry[];
  /** Playlist style colours from the active skin. */
  colors: {
    normal: string;
    current: string;
    normalbg: string;
    selectedbg: string;
    font: string;
  };
  onClose: () => void;
}

export default function ContextMenu({ x, y, items, colors, onClose }: ContextMenuProps) {
  // Close on any click outside the menu.
  useEffect(() => {
    const close = () => onClose();
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [onClose]);

  return createPortal(
    <div
      style={{
        position: "fixed",
        left: x,
        top: y,
        background: colors.normalbg,
        border: `1px solid ${colors.selectedbg}`,
        padding: "4px 0",
        zIndex: 1000,
        fontFamily: `"${colors.font}", system-ui, sans-serif`,
        fontSize: "12px",
        color: colors.normal,
        minWidth: "180px",
        boxShadow: "2px 2px 8px rgba(0,0,0,0.5)",
      }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {items.map((entry, i) => {
        if (entry === "separator") {
          return (
            <div
              key={`sep-${i}`}
              style={{ height: "1px", background: colors.selectedbg, margin: "4px 0" }}
            />
          );
        }
        return (
          <MenuItem
            key={entry.label}
            label={entry.label}
            disabled={entry.disabled}
            hoverBg={colors.selectedbg}
            onClick={() => {
              entry.onClick();
              onClose();
            }}
          />
        );
      })}
    </div>,
    document.body
  );
}

function MenuItem({
  label,
  onClick,
  hoverBg,
  disabled,
}: {
  label: string;
  onClick: () => void;
  hoverBg: string;
  disabled?: boolean;
}) {
  return (
    <div
      style={{
        padding: "6px 12px",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.4 : 1,
      }}
      onMouseEnter={(e) => {
        if (!disabled) (e.target as HTMLElement).style.background = hoverBg;
      }}
      onMouseLeave={(e) => {
        (e.target as HTMLElement).style.background = "transparent";
      }}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (!disabled) onClick();
      }}
    >
      {label}
    </div>
  );
}
