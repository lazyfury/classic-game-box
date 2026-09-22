// ---------------------------------------------------------------------------
// The function area -- the left rail.
//
// Modelled on the VS Code activity bar, with the one difference the design
// asked for: each item is an icon *with its name under it*, stacked
// vertically, rather than an icon that needs a tooltip to be understood.
// Chinese labels are short enough that they fit in a 76px rail, and a name
// that is always visible is one fewer thing to hover over.
//
// It is a real <nav> with real buttons because that is what it is: the only
// way to change what the middle column shows. The selection is carried by
// `aria-current` as well as by colour, so the accent fill is decoration and
// not the message.
//
// The peripheral indicators used to live at the bottom of this rail. They are
// in the status bar now, with words: an icon alone cannot say whether the pad
// is connected or merely expected, and the status bar is where readings go.
// ---------------------------------------------------------------------------

import { SECTIONS, type SectionId } from '../sections';

interface SidebarProps {
    active: SectionId;
    onSelect: (id: SectionId) => void;
}

export default function Sidebar({
    active,
    onSelect,
}: SidebarProps) {
    return (
        <nav className="sidebar" aria-label="功能区">
            <div className="rail">
                {SECTIONS.map((section) => {
                    const Icon = section.icon;
                    const selected = section.id === active;
                    return (
                        <button
                            key={section.id}
                            type="button"
                            className={selected ? 'rail-item rail-item-active' : 'rail-item'}
                            aria-current={selected ? 'page' : undefined}
                            onClick={() => onSelect(section.id)}
                            title={section.title}
                        >
                            <Icon className="rail-icon" size={20} strokeWidth={1.6} aria-hidden="true" />
                            <span className="rail-label">{section.label}</span>
                        </button>
                    );
                })}
            </div>
        </nav>
    );
}
