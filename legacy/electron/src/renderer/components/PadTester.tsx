// ---------------------------------------------------------------------------
// The pad tester: press a button, watch a chip light up.
//
// It exists because "L2" is a name the helper chose, and nobody knows which
// physical button that is until they press it. Without this the only way to
// find out is to bind a command to it and see whether something happens --
// which is a slow way to answer a question this simple.
//
// Two rows of lights, and the split is the one the whole input path is built
// on:
//
//   主机八个  what the game reads. Binding a command to one of these would
//            fire it every time the player pressed it, so the settings screen
//            does not offer them.
//   命令九个  the shoulders, the triggers, the stick clicks, the extra face
//            buttons and the home button. These are what a command may use,
//            and the ones the rows above are holding.
//
// It is a *report*, not a control: nothing here is clickable, because the thing
// being tested is the pad in the player's hand.
// ---------------------------------------------------------------------------

import {
    EXTRA_PAD_BUTTONS, GAMEPAD_SWITCHES,
    type ExtraPadButtonName, type GamepadButtonName, type PadButtonName,
} from '../../shared/api';
import type { PadSummary } from '../gamepad';

/** What a switch is called on the console. */
const SWITCH_LABELS: Record<GamepadButtonName, string> = {
    A: 'A', B: 'B', SELECT: 'Select', START: 'Start',
    UP: '上', DOWN: '下', LEFT: '左', RIGHT: '右',
};

/** The nine, short enough to fit a chip. */
const EXTRA_LABELS: Record<ExtraPadButtonName, string> = {
    L1: 'L1', R1: 'R1', L2: 'L2', R2: 'R2', L3: 'L3', R3: 'R3',
    FACE_X: 'X', FACE_Y: 'Y', GUIDE: '⌂',
};

function Lights({ pad, buttons, labels }: {
    pad: PadSummary;
    buttons: readonly PadButtonName[];
    labels: Record<string, string>;
}) {
    return (
        <div className="pad-lights">
            {buttons.map((button) => {
                const on = pad.buttons[button] === true;
                return (
                    <span
                        key={button}
                        className={on ? 'pad-light pad-light-on' : 'pad-light'}
                        data-name={button}
                        title={`${button}（协议里的名字）`}
                    >
                        {labels[button] ?? button}
                    </span>
                );
            })}
        </div>
    );
}

export default function PadTester({ pads }: { pads: readonly PadSummary[] }) {
    if (pads.length === 0) {
        return null;
    }

    return (
        <div className="pad-tester">
            {pads.map((pad) => (
                <div className="pad-tester-pad" key={pad.index}>
                    <Lights pad={pad} buttons={GAMEPAD_SWITCHES} labels={SWITCH_LABELS} />
                    <Lights pad={pad} buttons={EXTRA_PAD_BUTTONS} labels={EXTRA_LABELS} />
                </div>
            ))}
        </div>
    );
}
