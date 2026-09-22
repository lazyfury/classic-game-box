// ---------------------------------------------------------------------------
// One screenshot, at size, where the picture goes.
//
// The preview is a workspace pane: the game area swaps to it the way a tabbed
// admin screen swaps its body, by hiding one pane and showing another -- and
// hiding, not destroying. The pane underneath is `display: none` while this is
// up (see PlayPanel), so the canvas stays mounted, stays bound to the emulator,
// and stays out of the tab order.
//
// It is also the whole of the game's area, edge to edge, with nothing of its
// own taking room from the picture: the name and the buttons float over it on
// scrims, the way a full-screen viewer does. A 256x240 frame is worth every
// pixel of the biggest space in the window.
//
// While it is up the machine is paused (see `setPictureHidden` in useEmulator),
// which is what being hidden means: a game running behind a picture nobody can
// see is a game playing itself.
// ---------------------------------------------------------------------------

import { ChevronLeft, ChevronRight, FolderSearch, Image, Star, Trash2, X } from 'lucide-react';
import { useCallback, useEffect } from 'react';

import { libraryAssetUrl, type Screenshot } from '../../shared/api';
import { atEnd, atStart, neighbour } from '../gallery';

interface ScreenshotPreviewProps {
    /** The whole collection, so the arrows walk the same list the grid shows. */
    shots: readonly Screenshot[];
    /** Which of them is on screen. */
    index: number;
    onIndex: (index: number) => void;
    onReveal: (id: number) => void;
    onSetCover: (id: number) => void;
    onRemove: (id: number) => void;
    onClose: () => void;
}

function formatTaken(timestamp: number): string {
    return new Date(timestamp).toLocaleString(undefined, {
        year: 'numeric', month: 'numeric', day: 'numeric',
        hour: '2-digit', minute: '2-digit',
    });
}

export default function ScreenshotPreview({
    shots,
    index,
    onIndex,
    onReveal,
    onSetCover,
    onRemove,
    onClose,
}: ScreenshotPreviewProps) {
    const shot = shots[index];

    const step = useCallback((delta: number): void => {
        onIndex(neighbour(index, delta, shots.length));
    }, [index, shots.length, onIndex]);

    /**
     * Escape leaves, the arrows walk the list -- and nothing else gets through.
     *
     * `data-keyboard-captured` is the other half of this: it is what the
     * emulator's own listener looks for before deciding to ignore a key (see
     * input.ts). Two locks on one door, because the thing that happens when
     * both fail is a pad button stuck down: the arrow key-down would reach the
     * console, the key-up would not, and the game would resume with Mario
     * already walking.
     */
    useEffect(() => {
        const onKey = (event: KeyboardEvent): void => {
            if (event.key === 'Escape') {
                event.preventDefault();
                onClose();
            } else if (event.key === 'ArrowLeft') {
                event.preventDefault();
                step(-1);
            } else if (event.key === 'ArrowRight') {
                event.preventDefault();
                step(1);
            }
            // Everything is stopped, not only the keys above: a key that did
            // something to the page behind the picture would be a key that did
            // something invisible.
            event.stopPropagation();
        };
        window.addEventListener('keydown', onKey, true);
        return () => window.removeEventListener('keydown', onKey, true);
    }, [step, onClose]);

    if (shot === undefined) {
        return null;
    }

    return (
        <div
            className="preview"
            role="dialog"
            aria-label={`${shot.game} 的截图预览`}
            data-keyboard-captured="true"
        >
            <header className="preview-bar preview-head">
                <span className="preview-game" title={shot.gamePath}>{shot.game}</span>
                <span className="preview-when">{formatTaken(shot.createdAt)}</span>
                {shot.isCover && (
                    <span className="card-shots card-cover-flag" title="当前封面">
                        <Star size={10} />
                        封面
                    </span>
                )}
                <span className="preview-count">{index + 1} / {shots.length}</span>
                <button
                    type="button"
                    className="icon-button"
                    onClick={onClose}
                    title="关闭预览（Esc），游戏继续"
                    aria-label="关闭预览"
                    autoFocus
                >
                    <X size={14} />
                </button>
            </header>

            <div className="preview-stage">
                <img
                    className="preview-image"
                    src={libraryAssetUrl(shot.file)}
                    alt={`${shot.game} 的截图`}
                    draggable={false}
                />

                <button
                    type="button"
                    className="preview-step preview-prev"
                    onClick={() => step(-1)}
                    disabled={atStart(index)}
                    title="上一张（←）"
                    aria-label="上一张"
                >
                    <ChevronLeft size={22} />
                </button>

                <button
                    type="button"
                    className="preview-step preview-next"
                    onClick={() => step(1)}
                    disabled={atEnd(index, shots.length)}
                    title="下一张（→）"
                    aria-label="下一张"
                >
                    <ChevronRight size={22} />
                </button>
            </div>

            <footer className="preview-bar preview-actions">
                <span className="preview-note">预览中，游戏已暂停</span>
                {!shot.isCover && (
                    <button type="button" className="button" onClick={() => onSetCover(shot.id)}>
                        <Image size={13} />
                        设为封面
                    </button>
                )}
                <button
                    type="button"
                    className="button"
                    onClick={() => onReveal(shot.id)}
                    title="在文件浏览器中显示这个文件"
                >
                    <FolderSearch size={13} />
                    在访达中显示
                </button>
                <button
                    type="button"
                    className="button"
                    onClick={() => onRemove(shot.id)}
                    title="删除这张截图"
                >
                    <Trash2 size={13} />
                    删除
                </button>
            </footer>
        </div>
    );
}
