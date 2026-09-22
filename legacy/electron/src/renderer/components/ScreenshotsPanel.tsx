// ---------------------------------------------------------------------------
// Screenshots -- the middle column, in the 截图 section.
//
// Every picture the player has taken, newest first, with the game it came
// from under it. This is the collection, so it is one list across every game
// rather than a gallery per game: what somebody wants to find is the shot they
// just took, not a directory tree.
//
// The one thing that needs explaining is the difference between a screenshot
// and a cover. They are the same picture. A cover is a *flag* on one of a
// game's screenshots -- see src/main/library.ts for why that is better than a
// second column pointing at a second copy -- so "设为封面" moves one flag and
// nothing else, and deleting the cover just leaves the game with a different
// picture on its card.
//
// A thumbnail is a button: pressing it shows the picture at size in the play
// column, where the game's own picture goes -- see ScreenshotPreview, which is
// a component of that column and not of this one. The grid is for finding a
// picture; the preview is for looking at it, and a 256x240 frame drawn 90px
// wide is a colour rather than a picture.
// ---------------------------------------------------------------------------

import { Camera, FolderSearch, FolderOpen, Image, Star, Trash2 } from 'lucide-react';

import { libraryAssetUrl, type Screenshot } from '../../shared/api';

interface ScreenshotsPanelProps {
    screenshots: Screenshot[];
    /** Which picture the preview is showing, so the grid can mark it. */
    previewId: number | null;
    /** Show one at size, in the play column. */
    onPreview: (id: number) => void;
    /** Open the screenshots folder in the Finder. */
    onOpenFolder: () => void;
    /** Show one picture in the file browser, selected. */
    onReveal: (id: number) => void;
    onSetCover: (id: number) => void;
    onRemove: (id: number) => void;
}

function formatTaken(timestamp: number): string {
    const date = new Date(timestamp);
    const today = new Date();
    const sameDay = date.toDateString() === today.toDateString();
    return sameDay
        ? date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
        : date.toLocaleString(undefined, {
            month: 'numeric', day: 'numeric',
            hour: '2-digit', minute: '2-digit',
        });
}

export default function ScreenshotsPanel({
    screenshots,
    previewId,
    onPreview,
    onOpenFolder,
    onReveal,
    onSetCover,
    onRemove,
}: ScreenshotsPanelProps) {
    return (
        <section className="panel" aria-label="截图收藏">
            <header className="panel-head">
                <div className="panel-title">
                    <h2>截图收藏</h2>
                    <span className="panel-count">
                        {screenshots.length === 0 ? '空' : `${screenshots.length} 张`}
                    </span>
                    <span className="panel-title-actions">
                        <button
                            type="button"
                            className="icon-button"
                            onClick={onOpenFolder}
                            title="在访达中打开截图文件夹"
                            aria-label="在访达中打开截图文件夹"
                        >
                            <FolderOpen size={14} />
                        </button>
                    </span>
                </div>
            </header>

            <div className="panel-scroll">
                {screenshots.length === 0 && (
                    <div className="empty">
                        <Camera size={22} />
                        <p>还没有截图。</p>
                        <p className="empty-hint">
                            游玩时按 <kbd>F12</kbd>，或点画面下方的「截图」。
                            每个游戏的第一张截图会自动成为它的封面，也可以在下面手动指定。
                        </p>
                    </div>
                )}

                <ul className="cards cards-shots">
                    {screenshots.map((shot) => (
                        <li
                            key={shot.id}
                            className={[
                                'card-cell',
                                shot.isCover ? 'card-pinned' : '',
                                shot.id === previewId ? 'card-previewing' : '',
                            ].filter(Boolean).join(' ')}
                        >
                            <figure className="shot">
                                <button
                                    type="button"
                                    className="shot-open"
                                    onClick={() => onPreview(shot.id)}
                                    title="在右侧放大查看"
                                    aria-label={`放大查看 ${shot.game} 的截图`}
                                >
                                    <img
                                        className="shot-image"
                                        src={libraryAssetUrl(shot.file)}
                                        alt=""
                                        loading="lazy"
                                        draggable={false}
                                    />
                                </button>
                                <figcaption className="shot-caption">
                                    <span className="shot-game" title={shot.gamePath}>
                                        {shot.game}
                                    </span>
                                    <span className="shot-when">{formatTaken(shot.createdAt)}</span>
                                </figcaption>
                            </figure>

                            <span className="card-actions">
                                {shot.isCover ? (
                                    <span className="card-shots card-cover-flag" title="当前封面">
                                        <Star size={10} />
                                        封面
                                    </span>
                                ) : (
                                    <button
                                        type="button"
                                        className="icon-button"
                                        onClick={() => onSetCover(shot.id)}
                                        title="设为封面"
                                        aria-label="设为封面"
                                    >
                                        <Image size={12} />
                                    </button>
                                )}
                                <button
                                    type="button"
                                    className="icon-button"
                                    onClick={() => onReveal(shot.id)}
                                    title="在访达中显示"
                                    aria-label="在访达中显示"
                                >
                                    <FolderSearch size={12} />
                                </button>
                                <button
                                    type="button"
                                    className="icon-button"
                                    onClick={() => onRemove(shot.id)}
                                    title="删除截图"
                                    aria-label="删除截图"
                                >
                                    <Trash2 size={12} />
                                </button>
                            </span>
                        </li>
                    ))}
                </ul>
            </div>

        </section>
    );
}
