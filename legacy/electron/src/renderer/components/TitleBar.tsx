// ---------------------------------------------------------------------------
// The unified title bar.
//
// macOS with `titleBarStyle: 'hiddenInset'` keeps the real traffic lights and
// lets the page paint the rest of the strip. That leaves two obligations:
//
//   1. Leave room on the left for the lights. They are drawn by the window
//      server, at a fixed offset, and a button under them cannot be clicked;
//      the 78px in the stylesheet is that room.
//   2. Say `-webkit-app-region: drag` on the strip, or the window cannot be
//      moved. Anything clickable inside has to opt out with `no-drag`.
//
// The actions on the right are icons because a macOS toolbar is icons, and
// they carry tooltips because an icon without one is a guess. The tooltip is
// Radix's: it is headless, so the look is still the stylesheet's, and it
// brings the parts that are easy to get wrong -- hover intent, a delay, a
// portal so it is not clipped, and `aria-describedby` on the trigger.
// ---------------------------------------------------------------------------

import * as Tooltip from '@radix-ui/react-tooltip';
import { FolderOpen, Info, RefreshCw } from 'lucide-react';
import type { ReactNode } from 'react';

interface ToolbarActionProps {
    label: string;
    onClick: () => void;
    disabled?: boolean;
    children: ReactNode;
}

function ToolbarAction({ label, onClick, disabled = false, children }: ToolbarActionProps) {
    return (
        <Tooltip.Root>
            <Tooltip.Trigger asChild>
                <button
                    type="button"
                    className="toolbar-button"
                    onClick={onClick}
                    disabled={disabled}
                    aria-label={label}
                >
                    {children}
                </button>
            </Tooltip.Trigger>
            <Tooltip.Portal>
                <Tooltip.Content className="tooltip" side="bottom" align="center" sideOffset={6}>
                    {label}
                </Tooltip.Content>
            </Tooltip.Portal>
        </Tooltip.Root>
    );
}

interface TitleBarProps {
    /** The section on show, and the game loaded in it, if any. */
    section: string;
    subtitle: string;
    /** True while the library is being re-read. */
    busy: boolean;
    onRefresh: () => void;
    onOpenFolder: () => void;
    onAbout: () => void;
}

export default function TitleBar({
    section,
    subtitle,
    busy,
    onRefresh,
    onOpenFolder,
    onAbout,
}: TitleBarProps) {
    return (
        <header className="titlebar">
            {/* Reserved for the traffic lights. Not a fake set: drawing our
                own would double them up on a real window. */}
            <div className="titlebar-lights" aria-hidden="true" />

            <div className="titlebar-titles">
                <span className="titlebar-app">Classic Game Box</span>
                <span className="titlebar-sub">{subtitle || section}</span>
            </div>

            <div className="titlebar-drag" />

            <div className="titlebar-actions">
                <ToolbarAction label="重新读取游戏库" onClick={onRefresh} disabled={busy}>
                    <RefreshCw size={15} className={busy ? 'spin' : undefined} />
                </ToolbarAction>
                <ToolbarAction label="在访达中显示游戏文件夹" onClick={onOpenFolder}>
                    <FolderOpen size={15} />
                </ToolbarAction>
                <ToolbarAction label="关于" onClick={onAbout}>
                    <Info size={15} />
                </ToolbarAction>
            </div>
        </header>
    );
}
