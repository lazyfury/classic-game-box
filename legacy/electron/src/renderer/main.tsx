import { Profiler, StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import { logPageTiming, watchLongTasks } from './bootTiming';
import { bootLog, setBootOrigin } from '../shared/boot';
import './styles.css';

// Adopt the shared start up clock before anything else is logged, so the
// renderer's lines sit on the same timeline as the TypeScript compile and the
// Vite server that happened before it. See src/shared/boot.ts.
setBootOrigin(window.fc?.bootT0 ?? 0);

bootLog('renderer', 'script started', `${location.href} (readyState=${document.readyState})`);
watchLongTasks();

const container = document.getElementById('root');
if (container === null) {
    throw new Error('index.html has no #root');
}

bootLog('renderer', 'react render called');

// React's own stopwatch around the first paint. The long task observer above
// says the main thread was busy for several seconds; this says how much of
// that React thinks was its own render and commit, which is the difference
// between "a component is slow" and "the browser is busy with something
// else". Capped at a handful of commits, because once the shell is up it
// re-renders four times a second and every one of those is uninteresting.
let commitsLogged = 0;
createRoot(container).render(
    <StrictMode>
        <Profiler
            id="app"
            onRender={(id, phase, actualDuration, baseDuration) => {
                if (commitsLogged >= 8) {
                    return;
                }
                commitsLogged += 1;
                bootLog(
                    'renderer',
                    `profiler ${id}`,
                    `${phase} actual=${Math.round(actualDuration)}ms base=${Math.round(baseDuration)}ms`,
                );
            }}
        >
            <App />
        </Profiler>
    </StrictMode>,
);

// The first animation frame after render() is the first moment the page can
// paint, so this is the renderer's equivalent of the main process's
// `ready-to-show`. The resource entries are final by then too.
requestAnimationFrame(() => {
    bootLog('renderer', 'first animation frame (painted)');
    logPageTiming();
});
