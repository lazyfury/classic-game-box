// ---------------------------------------------------------------------------
// The settings index -- the middle column, and the only thing in it.
//
// It is deliberately not a settings panel. The settings themselves are in the
// play column, in tabs (see SettingsPanes): the values want room, and the
// middle column is the narrow one. What is left for it to do is say what there
// is to set, which is a list.
//
// So this is a list of groups, each with one line of explanation, and clicking
// one switches the right-hand tab. The chosen one is marked the way the rail
// marks a section -- fill and colour, with `aria-current` so it is not only
// colour.
// ---------------------------------------------------------------------------

import { ChevronRight } from 'lucide-react';

import { SETTINGS_GROUPS, type SettingsGroupId } from '../settings';

interface SettingsIndexProps {
    active: SettingsGroupId;
    onSelect: (id: SettingsGroupId) => void;
}

export default function SettingsIndex({ active, onSelect }: SettingsIndexProps) {
    return (
        <section className="panel" aria-label="设置">
            <header className="panel-head">
                <div className="panel-title">
                    <h2>设置</h2>
                    <span className="panel-count">在右边</span>
                </div>
            </header>

            <div className="panel-scroll">
                <ul className="settings-index">
                    {SETTINGS_GROUPS.map((group) => (
                        <li key={group.id}>
                            <button
                                type="button"
                                className="settings-index-item"
                                aria-current={group.id === active ? 'true' : undefined}
                                onClick={() => onSelect(group.id)}
                            >
                                <span className="settings-index-label">
                                    {group.label}
                                    <ChevronRight size={13} aria-hidden="true" />
                                </span>
                                <span className="settings-index-hint">{group.hint}</span>
                            </button>
                        </li>
                    ))}
                </ul>
            </div>
        </section>
    );
}
