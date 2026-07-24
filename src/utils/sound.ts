// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// 报错提示音：用 Web Audio API 合成一段短促的下行双音，不依赖外部音频文件。
// 是否播放由 composables/useNotify.ts 根据用户设置和错误等级判断。

let audioCtx: AudioContext | null = null;

const getAudioContext = (): AudioContext | null => {
  if (typeof window === "undefined") return null;
  const Ctor = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  if (!audioCtx) audioCtx = new Ctor();
  return audioCtx;
};

export const playErrorSound = (): void => {
  const ctx = getAudioContext();
  if (!ctx) return;
  // 浏览器要求先有一次用户交互才允许播放音频；报错发生时上下文大概率已
  // 因为之前的点击操作被激活过，这里补一次 resume 兜底处于 suspended 的情况。
  if (ctx.state === "suspended") {
    void ctx.resume().catch(() => {
      // 尚未获得用户交互授权时保持静音，不影响错误弹窗本身。
    });
  }

  const now = ctx.currentTime;
  const playTone = (freq: number, start: number, duration: number) => {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.setValueAtTime(freq, now + start);
    gain.gain.setValueAtTime(0, now + start);
    gain.gain.linearRampToValueAtTime(0.18, now + start + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.001, now + start + duration);
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start(now + start);
    osc.stop(now + start + duration);
  };
  // 下行双音（880Hz -> 587Hz），短促、克制，不刺耳。
  playTone(880, 0, 0.12);
  playTone(587, 0.13, 0.18);
};
