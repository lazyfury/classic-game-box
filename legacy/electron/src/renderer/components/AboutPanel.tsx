// ---------------------------------------------------------------------------
// About -- the middle column, in the 关于 section.
//
// This project is a computer science course that happens to produce an
// emulator, so the panel that says what it is should say what it teaches: the
// chain from a byte in a ROM file to a pixel on this screen, which is the
// whole of it. The list of stages is not decoration -- it is the syllabus, and
// every stage of it is implemented and tested in this repository.
// ---------------------------------------------------------------------------

import { Check } from 'lucide-react';

const CHAIN: readonly { stage: string; what: string }[] = [
    { stage: 'ROM', what: '.nes 文件，iNES 文件头 + PRG/CHR' },
    { stage: '机器码', what: '字节流，反汇编成助记符' },
    { stage: 'Opcode', what: '151 条 6502 指令，256 项周期表' },
    { stage: 'CPU', what: 'A / X / Y / SP / PC / P，取指—译码—执行' },
    { stage: '总线', what: '$0000-$FFFF 地址译码、镜像、open bus' },
    { stage: '卡带', what: 'Mapper 0-249，分页、IRQ、扩展区寄存器' },
    { stage: 'PPU', what: '背景、精灵、滚动、调色板、256×240' },
    { stage: '帧缓冲', what: '61440 个 0x00RRGGBB 像素' },
    { stage: 'Canvas', what: 'B,G,R,X → R,G,B,A，整数倍缩放' },
    { stage: '屏幕', what: '一块 Metal/Canvas 表面的像素' },
];

export default function AboutPanel() {
    return (
        <section className="panel" aria-label="关于">
            <header className="panel-head">
                <div className="panel-title">
                    <h2>关于</h2>
                    <span className="panel-count">Classic Game Box 0.1.0</span>
                </div>
            </header>

            <div className="panel-scroll">
                <section className="group">
                    <h3>这是什么</h3>
                    <p className="prose">
                        一个用 C++20 从零写的 FC / NES 模拟器，核心编译成 WebAssembly，
                        外面套一层 Electron 外壳。核心不知道 UI 的存在，UI 也不模拟任何硬件。
                    </p>
                </section>

                <section className="group">
                    <h3>从字节到像素</h3>
                    <ol className="chain">
                        {CHAIN.map((step) => (
                            <li key={step.stage}>
                                <span className="chain-tick" aria-hidden="true">
                                    <Check size={11} />
                                </span>
                                <span className="chain-stage">{step.stage}</span>
                                <span className="chain-what">{step.what}</span>
                            </li>
                        ))}
                    </ol>
                </section>

                <section className="group">
                    <h3>技术</h3>
                    <dl className="rows">
                        <div className="row">
                            <dt>核心</dt>
                            <dd>C++20 · GoogleTest · Emscripten</dd>
                        </div>
                        <div className="row">
                            <dt>外壳</dt>
                            <dd>Electron · React · TypeScript · Vite</dd>
                        </div>
                        <div className="row">
                            <dt>原生前端</dt>
                            <dd>Swift · SwiftUI · Metal · CoreAudio</dd>
                        </div>
                        <div className="row">
                            <dt>图标</dt>
                            <dd>lucide</dd>
                        </div>
                    </dl>
                </section>

                <section className="group">
                    <h3>快捷键</h3>
                    <p className="prose">
                        完整列表在「设置」里。最常用的三个：<kbd>Esc</kbd> 暂停、
                        <kbd>R</kbd> 重置、按住 <kbd>Backspace</kbd> 倒带。
                    </p>
                </section>
            </div>
        </section>
    );
}
