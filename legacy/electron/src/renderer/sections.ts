// ---------------------------------------------------------------------------
// What the left rail can be showing.
//
// One place, so the rail, the window title and the middle column cannot
// disagree about what the middle column is. The icon lives here too, and not
// in the component, because a section without an icon is a section nobody
// built yet rather than a layout decision.
//
// There is no "recently played" section. It used to be one, and it was the
// same list as the library filtered to the games with a play date -- which the
// library itself now does properly, with an order for it (see SortKey), and
// without a second copy of the same rule.
// ---------------------------------------------------------------------------

import { Camera, Info, Library, Pin, Save, Settings2, Sparkles, type LucideIcon } from 'lucide-react';

export type SectionId =
    | 'library' | 'pinned' | 'screenshots' | 'saves' | 'cheats' | 'settings' | 'about';

export interface Section {
    id: SectionId;
    /** What the rail says. */
    label: string;
    /** What the window title bar says, for the benefit of the screenshot in a
     *  bug report. */
    title: string;
    icon: LucideIcon;
}

export const SECTIONS: readonly Section[] = [
    { id: 'library', label: '游戏库', title: '游戏库', icon: Library },
    { id: 'pinned', label: '置顶', title: '置顶游戏', icon: Pin },
    { id: 'screenshots', label: '截图', title: '截图收藏', icon: Camera },
    { id: 'saves', label: '存档', title: '存档', icon: Save },
    { id: 'cheats', label: '金手指', title: '金手指', icon: Sparkles },
    { id: 'settings', label: '设置', title: '设置', icon: Settings2 },
    { id: 'about', label: '关于', title: '关于', icon: Info },
];

export const SECTION_BY_ID: Readonly<Record<SectionId, Section>> = Object.freeze(
    Object.fromEntries(SECTIONS.map((section) => [section.id, section])) as Record<SectionId, Section>,
);
