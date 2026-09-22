// ---------------------------------------------------------------------------
// A tabbed view: one pane showing, every pane mounted.
//
// The same idea as a browser's tabs or an admin console's workspace -- the pane
// you are not looking at is still there, with its state and its elements, and
// switching is a matter of which pane the browser draws. What it is *not* is a
// tab *strip*: there is nothing to click, because which tab is showing is not a
// choice the player makes here. It is a fact about the application -- a
// screenshot is open, or it is not -- and the control that changes it is the
// thumbnail that was clicked and the ✕ that closes it.
//
// That distinction is the whole reason this component exists rather than a
// conditional render. The console pane holds a canvas that is bound to the
// emulator once, when the WebAssembly module is built: unmounting it to show a
// picture would rebuild the module and power the console off, so the pane that
// is not showing has to be hidden with CSS and left in the DOM.
//
// `display: none` is what hides it -- not `opacity: 0`, which leaves a pane that
// can still be clicked, and not `visibility: hidden`, which leaves one that can
// still be tabbed into. The cost is that a hidden pane has no box, which is why
// the console's `usePixelScale` ignores a container measuring under a pixel and
// re-measures when the box comes back.
// ---------------------------------------------------------------------------

import type { ReactNode } from 'react';

export interface TabPane {
    /** Identifies the pane. The one whose id equals `active` is the one shown. */
    id: string;
    content: ReactNode;
}

interface TabsViewProps {
    /** Which pane to show. An id no pane has shows none of them. */
    active: string;
    /** Every pane, in the order they are laid out. All of them are rendered. */
    panes: readonly TabPane[];
}

export default function TabsView({ active, panes }: TabsViewProps) {
    return (
        <div className="tabs">
            {panes.map((pane) => (
                <section
                    key={pane.id}
                    className={pane.id === active ? 'tab-pane' : 'tab-pane tab-pane-hidden'}
                    data-tab={pane.id}
                >
                    {pane.content}
                </section>
            ))}
        </div>
    );
}
