// ---------------------------------------------------------------------------
// What the settings screen is made of.
//
// The rail picks 设置, and then there are two columns left to fill: the middle
// one is the index -- what there is to set -- and the play column shows the
// group that is chosen. That is the pattern the rest of the application
// already uses for the play column (`TabsView`: one pane shown, the rest
// mounted), so settings are a tab per group rather than one long scroll.
//
// The list lives here rather than in either component because both need it:
// the index draws the labels, and the panes render the content in the same
// order. A group added to one and not the other would be a tab that shows
// nothing or a pane nothing can reach.
// ---------------------------------------------------------------------------

export type SettingsGroupId = 'input' | 'display' | 'cores' | 'library';

export interface SettingsGroup {
    id: SettingsGroupId;
    /** What the index calls it, and what the pane's header says. */
    label: string;
    /** One line under it, so the index is readable without clicking. */
    hint: string;
}

export const SETTINGS_GROUPS: readonly SettingsGroup[] = Object.freeze([
    {
        id: 'input',
        label: '输入',
        hint: '键盘怎么驱动两个手柄，哪个手柄是哪一位玩家，以及暂停、截图、存档这些命令绑在哪个键和哪个键位上',
    },
    {
        id: 'display',
        label: '画面',
        hint: '扫描线，以及画面、音频和运行状态的实际数字',
    },
    {
        id: 'cores',
        label: '核心',
        hint: '每个机种用哪个模拟器核心',
    },
    {
        id: 'library',
        label: '游戏库',
        hint: '库在哪、有多少游戏和截图，以及换一个库',
    },
]);

/** Which group the screen opens on: the one this was built for. */
export const DEFAULT_SETTINGS_GROUP: SettingsGroupId = 'input';

/** The group with this id, or the first one, so a pane always has a header. */
export function settingsGroup(id: SettingsGroupId): SettingsGroup {
    return SETTINGS_GROUPS.find((group) => group.id === id) ?? SETTINGS_GROUPS[0];
}
