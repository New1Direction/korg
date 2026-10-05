// Synthesised sound design for the KORG trailer — no samples, no dependencies.
// Writes out/korg-trailer.wav (44.1 kHz, 16-bit stereo). Every cue is tied to a
// timestamp in index.html; the silences (11.0, 30.4, 50.0) are gated on purpose.
import fs from "fs"; import path from "path"; import { fileURLToPath } from "url";
const here = path.dirname(fileURLToPath(import.meta.url));
const SR = 44100, DUR = 60, N = SR * DUR, TAU = Math.PI * 2;
const dryL = new Float32Array(N), dryR = new Float32Array(N), wet = new Float32Array(N);
let seed = 7; const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; };
const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const sstep = (a, b, x) => { const t = clamp((x - a) / (b - a)); return t * t * (3 - 2 * t); };
const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);

function put(i, v, pan, w) {
  if (i < 0 || i >= N) return;
  dryL[i] += v * (1 - Math.max(0, pan)) ; dryR[i] += v * (1 + Math.min(0, pan));
  wet[i] += v * w;
}
const wavef = { sine: (p) => Math.sin(TAU * p), saw: (p) => 2 * (p - Math.floor(p)) - 1, square: (p) => (p % 1) < .5 ? 1 : -1, tri: (p) => 1 - 4 * Math.abs((p % 1) - .5) };

/* hit/decay or swell oscillator with exponential pitch sweep */
function tone({ t, dur, f0, f1 = f0, g = .3, wave = "sine", a = .004, d = 3, w = .25, pan = 0, swell = 0, fm = 0, fmr = 2, lp = 0 }) {
  const n = Math.floor(dur * SR), s0 = Math.floor(t * SR); let ph = 0, y = 0;
  const k = lp ? 1 - Math.exp(-TAU * lp / SR) : 1;
  for (let i = 0; i < n; i++) {
    const u = i / n, tt = i / SR, f = f0 * Math.pow(f1 / f0, u);
    ph += f / SR + (fm ? fm * Math.sin(TAU * f * fmr * tt) / SR * 0 : 0);
    let o = fm ? Math.sin(TAU * ph + fm * Math.sin(TAU * ph * fmr)) : wavef[wave](ph);
    const env = swell ? Math.pow(Math.sin(Math.PI * Math.pow(u, swell)), 2) : Math.min(1, tt / a) * Math.exp(-tt * d);
    y += k * (o - y); put(s0 + i, (lp ? y : o) * env * g, pan, w);
  }
}
function noise({ t, dur, g = .3, lp0 = 8000, lp1 = lp0, hp = 0, a = .002, d = 5, w = .25, swell = 0, pan = 0 }) {
  const n = Math.floor(dur * SR), s0 = Math.floor(t * SR); let y = 0, h = 0;
  for (let i = 0; i < n; i++) {
    const u = i / n, tt = i / SR, fc = lp0 * Math.pow(lp1 / lp0, u), k = 1 - Math.exp(-TAU * fc / SR);
    const x = rnd() * 2 - 1; y += k * (x - y); let o = y;
    if (hp) { h += (1 - Math.exp(-TAU * hp / SR)) * (y - h); o = y - h; }
    const env = swell ? Math.pow(Math.sin(Math.PI * Math.pow(u, swell)), 2) : Math.min(1, tt / a) * Math.exp(-tt * d);
    put(s0 + i, o * env * g, pan, w);
  }
}
function pad(t, dur, notes, g, w = .5, lfo = .15) {   // slow detuned saw/sine pad
  for (const m of notes) for (const det of [-.07, .07]) {
    const n = Math.floor(dur * SR), s0 = Math.floor(t * SR), f = mtof(m) * Math.pow(2, det / 12); let ph = rnd(), y = 0;
    const ph2 = rnd() * TAU;
    for (let i = 0; i < n; i++) {
      const tt = i / SR, u = i / n; ph += f / SR;
      const env = Math.min(1, tt / Math.min(2.5, dur * .4)) * Math.min(1, (dur - tt) / Math.min(2.5, dur * .4)) * (.8 + .2 * Math.sin(TAU * lfo * tt + ph2));
      const o = .6 * Math.sin(TAU * ph) + .25 * wavef.saw(ph) * .5 + .15 * Math.sin(TAU * ph * 2);
      y += .02 * (o - y); put(s0 + i, (o * .4 + y * .6) * env * g, det * 3, w);
    }
  }
}
const kick = (t, g = .8) => { tone({ t, dur: .45, f0: 150, f1: 42, g, d: 7, w: .05 }); noise({ t, dur: .03, g: g * .25, lp0: 6000, d: 90, w: 0 }); };
const hat = (t, g = .12) => noise({ t, dur: .06, g, lp0: 14000, hp: 6000, d: 60, w: .05, pan: rnd() * .6 - .3 });
const tick = (t, g = .05, f = 3200) => tone({ t, dur: .02, f0: f, g, d: 150, w: .1, pan: rnd() - .5 });
const ping = (t, m, g = .07, w = .6) => tone({ t, dur: 1.6, f0: mtof(m), g, d: 3.2, w, pan: rnd() - .5, a: .003 });
function impact(t, g = 1, o = {}) {
  tone({ t, dur: 2.2, f0: o.f0 || 95, f1: 30, g: .85 * g, d: 1.9, w: .12 });
  noise({ t, dur: 1.4, g: .55 * g, lp0: 5000, lp1: 140, d: 3.2, w: .55 });
  noise({ t, dur: .08, g: .6 * g, lp0: 12000, d: 40, w: .1 });
  for (const f of [196, 293, 440, 587]) tone({ t, dur: 2.4, f0: f * (o.dark ? .5 : 1), g: .05 * g, d: 2.2, w: .7 });
  if (o.bell) tone({ t, dur: 3, f0: 880, g: .1 * g, d: 1.4, fm: 2.2, fmr: 3.5, w: .7 });
}
const riser = (t, dur, g = .3, f1 = 6000) => { noise({ t, dur, g, lp0: 300, lp1: f1, swell: 2.2, w: .4 }); tone({ t, dur, f0: 80, f1: 880, g: g * .35, swell: 2.2, w: .4 }); };

/* ---------------------------------------------------------------- the cues */
// 0:00 grok — mechanical hum, glitch, fragmentation
tone({ t: 0, dur: 3.8, f0: 55, g: .3, a: 1.2, d: .15, w: .2 }); tone({ t: 0, dur: 3.8, f0: 110.4, g: .12, a: 1.5, d: .1, w: .2 });
tone({ t: 0.3, dur: 3.4, f0: 100, g: .06, wave: "saw", lp: 260, a: 1, d: .1, w: .1 });
for (let i = 0; i < 70; i++) { const tt = 1.55 + Math.pow(i / 70, .8) * 1.0; noise({ t: tt, dur: .018 + rnd() * .03, g: .12 + rnd() * .12, lp0: 3000 + rnd() * 9000, d: 40, w: .1, pan: rnd() - .5 }); }
tone({ t: 1.6, dur: .9, f0: 880, f1: 1760, g: .03, wave: "square", a: .01, d: 1.5, w: .2 });
noise({ t: 2.15, dur: .95, g: .22, lp0: 500, lp1: 9000, swell: 1.3, w: .4 });
for (let i = 0; i < 160; i++) { const tt = 2.2 + rnd() * 1.6; tone({ t: tt, dur: .03, f0: 800 + rnd() * 5200, g: .035, d: 90, w: .35, pan: rnd() * 2 - 1 }); }
tone({ t: 3.0, dur: .78, f0: 200, f1: 1500, g: .1, swell: 1.4, w: .4 });
// 0:04 KORG — impact
impact(4.1, 1.0); tone({ t: 4.1, dur: .5, f0: 50, f1: 50, g: .22, wave: "square", lp: 220, d: 5, w: .1 });
for (let i = 0; i < 17; i++) tick(4.5 + i / 36, .035, 2400);
tone({ t: 4.2, dur: 1.9, f0: 55, g: .22, d: 1.1, a: .02 });
// 0:06 the agent works — tension build, rhythmic cuts
const cuts = [6.0, 6.62, 7.1, 7.5, 7.85, 8.2, 8.5, 8.78, 9.05, 9.3, 9.55, 9.8, 10.0, 10.2];
tone({ t: 5.8, dur: 5.15, f0: 55, g: .22, wave: "saw", lp: 700, a: 1.2, d: .05, w: .15 });
for (const c of cuts) { tick(c, .09, 1800 + rnd() * 1200); tone({ t: c, dur: .12, f0: 120, f1: 60, g: .14, d: 20, w: .08 }); }
for (let i = 0; i < 52; i++) tick(6.0 + i * .085 + rnd() * .03, .03, 4200 + rnd() * 1500);
for (const a of [6.55, 7.55, 8.55, 9.55]) { tone({ t: a, dur: .3, f0: 180, f1: 55, g: .5, d: 9, w: .2 }); noise({ t: a, dur: .1, g: .25, lp0: 7000, lp1: 800, d: 22, w: .3 }); }
for (let i = 0; i < 16; i++) hat(8.4 + i * .1, .05 + i * .004);
riser(9.4, 1.5, .28, 9000);
// 0:10.9 ERROR — one violent hit, then the world stops
tone({ t: 10.9, dur: .3, f0: 70, f1: 38, g: .95, d: 12, w: .05 }); tone({ t: 10.9, dur: .3, f0: 60, g: .35, wave: "square", lp: 400, d: 14, w: 0 });
noise({ t: 10.9, dur: .25, g: .7, lp0: 9000, lp1: 400, d: 14, w: .1 });
// 0:12 dive into the machine
noise({ t: 12.0, dur: 1.8, g: .3, lp0: 90, lp1: 3500, swell: 1.2, w: .5 }); tone({ t: 12.0, dur: 1.8, f0: 38, f1: 420, g: .3, swell: 1.4, w: .4 });
for (let i = 0; i < 60; i++) tone({ t: 12.9 + rnd() * .9, dur: .03, f0: 600 + rnd() * 4000, g: .04, d: 80, w: .4, pan: rnd() * 2 - 1 });
impact(13.75, .55, { dark: true });
// 0:13.8 inside the history
pad(13.6, 5.2, [45, 52, 57, 60], .075); 
for (let i = 0; i < 46; i++) { const tt = 14.0 + i * .26 + rnd() * .1, sc = [69, 72, 76, 79, 81, 84]; ping(tt, sc[Math.floor(rnd() * sc.length)] + (rnd() < .3 ? 12 : 0), .05 + .02 * rnd()); }
for (let b = 0; b < 4; b++) kick(15.2 + b * 1.0 - 0, .18 + b * .08);
riser(16.9, 1.2, .3, 8000);
impact(18.0, .7);
// 0:18 the chain — decode chirps, slow pad
pad(18.0, 8.2, [45, 52, 57, 61, 64], .08);
for (let i = 0; i < 90; i++) tick(18.6 + i * .06 + rnd() * .02, .028, 2200 + rnd() * 3000);
for (let b = 0; b < 6; b++) kick(18.2 + b * 1.0, .25);
for (const [tt, m] of [[18.0, 57], [19.4, 60], [20.8, 64], [22.2, 67]]) ping(tt, m, .09, .7);
riser(22.2, 2.0, .34, 9000);
impact(24.2, .8, { bell: true });
riser(24.3, 1.7, .4, 12000); tone({ t: 24.3, dur: 1.7, f0: 110, f1: 880, g: .15, swell: 2, w: .4 });
// 0:26 REWIND — tape whirr tied to the visual playhead, ticks on every node it passes
impact(26.0, 1.0); noise({ t: 26.0, dur: .6, g: .4, lp0: 12000, lp1: 800, d: 6, w: .3 });
{
  const t0 = 26.35, dur = 4.0, n = Math.floor(dur * SR), s0 = Math.floor(t0 * SR); let ph = 0, ph2 = 0, y = 0;
  for (let i = 0; i < n; i++) {
    const u = i / n, sp = Math.pow(1 - u, 1.4), f = 90 + 2400 * sp, env = Math.min(1, (i / SR) / .05) * Math.min(1, (dur - i / SR) / .03);
    ph += f / SR; ph2 += f * 1.505 / SR; const x = rnd() * 2 - 1; y += .15 * (x - y);
    put(s0 + i, ((2 * (ph % 1) - 1) * .12 + Math.sin(TAU * ph2) * .09 + y * .28 * sp) * env, 0, .25);
  }
  for (let idx = 149; idx >= 107; idx--) { const u = 1 - Math.pow(1 - (150 - idx) / 43, 1 / 2.4); const tt = 26.35 + 4 * u, sp = Math.pow(1 - u, 1.4); tick(tt, .05 + .1 * sp, 900 + 3000 * sp); }
  tone({ t: 26.4, dur: 4, f0: 60, g: .2, wave: "saw", lp: 300, a: .3, d: .1, w: .1 });
}
tone({ t: 30.35, dur: .08, f0: 70, f1: 40, g: .6, d: 30, w: 0 });          // the dead stop
// 0:31 silence, then the fork lights
pad(31.25, 3.4, [57, 64, 69, 76], .06, .7); ping(31.6, 81, .08); ping(31.9, 88, .05); riser(31.8, .55, .22, 10000);
impact(32.35, .85, { bell: true });
// 0:34 the other future
for (let i = 0; i < 40; i++) { if (i % 2 === 0) kick(34.0 + i * .25 + (i > 3 ? 0 : 0), .22); hat(34.125 + i * .25, .06); }
ping(34.2, 69, .14); ping(35.2, 72, .14); ping(35.22, 76, .09);
for (const m of [69, 73, 76, 81]) ping(36.2, m, .12, .8); noise({ t: 36.2, dur: .5, g: .22, lp0: 12000, lp1: 3000, d: 6, w: .5 });
{ const sc = [69, 71, 73, 76, 78, 81, 83, 85]; for (let k = 0; k < 28; k++) ping(36.6 + k * .11, sc[k % sc.length] + (k > 8 ? 12 : 0), .045, .7); }
pad(34.0, 9.2, [45, 57, 64, 69, 73], .09, .6);
riser(40.0, 3.0, .38, 14000); tone({ t: 40, dur: 3, f0: 90, f1: 1400, g: .12, swell: 2, w: .5 });
// 0:43 the machinery — cut-driven
for (let c = 0; c < 6; c++) { const t0 = 43 + c; kick(t0, .55); noise({ t: t0, dur: .1, g: .22, lp0: 9000, hp: 1500, d: 22, w: .2 }); for (let k = 1; k < 8; k++) hat(t0 + k * .125, .04 + .015 * (k % 2)); tick(t0 + .5, .06, 1200); }
for (let c = 0; c < 6; c++) pad(43 + c, 1.2, [45 + (c % 2) * 3, 57, 64, 69], .1, .4, .5);
tone({ t: 43, dur: 6, f0: 55, g: .28, wave: "saw", lp: 500, a: .2, d: .05, w: .1 });
riser(47.5, 1.5, .3, 12000);
[0, .2, .4, .6].forEach((o, i) => { impact(49.0 + o, .45 + .1 * i, { f0: 80 + 10 * i }); tone({ t: 49.0 + o, dur: .4, f0: mtof(57 + i * 3), g: .16, wave: "saw", lp: 2400, d: 4, w: .4 }); });
// 0:50 black, the two lines
tone({ t: 50.6, dur: 5.6, f0: 55, g: .3, a: 2.2, d: .05, w: .3 }); tone({ t: 50.6, dur: 5.6, f0: 82.4, g: .14, a: 2.5, d: .05, w: .3 });
tone({ t: 50.65, dur: 1.4, f0: 70, f1: 34, g: .35, d: 2.4, w: .3 });
tone({ t: 52.15, dur: .25, f0: 400, f1: 80, g: .12, d: 12, w: .5 });
pad(53.0, 3.3, [45, 52, 57, 64], .09, .8); riser(53.4, 2.4, .22, 7000);
ping(54.0, 81, .09, .8); ping(54.3, 88, .06, .8);
// 0:56 KORG
impact(56.0, .6, { dark: true }); pad(56.0, 4.0, [45, 52, 57, 61, 69], .1, .8);
for (let i = 0; i < 17; i++) tick(57.0 + i / 20, .03, 2200);
ping(57.7, 69, .08, .8); ping(57.7, 76, .06, .8);

/* ------------------------------------------------------------------ master */
const gates = [[3.78, 4.1], [11.02, 12.0], [30.4, 31.25], [50.0, 50.6]];
const gate = (t) => { let g = 1; for (const [a, b] of gates) { g *= 1 - (sstep(a - .012, a, t) - sstep(b - .012, b, t)); } return g; };
const sustained = (t) => 1;                                                         // (hook for per-section trims)
// Schroeder-style hall on the wet bus, decorrelated L/R
function reverb(src, delays, fb, ap) {
  const out = new Float32Array(N), bufs = delays.map((d) => new Float32Array(d)), pos = delays.map(() => 0), lpv = delays.map(() => 0), apb = ap.map((d) => new Float32Array(d)), app = ap.map(() => 0);
  for (let i = 0; i < N; i++) {
    let acc = 0;
    for (let k = 0; k < bufs.length; k++) { const b = bufs[k], p = pos[k], y = b[p]; lpv[k] += .35 * (y - lpv[k]); b[p] = src[i] + lpv[k] * fb; pos[k] = (p + 1) % b.length; acc += y; }
    acc *= .25;
    for (let k = 0; k < apb.length; k++) { const b = apb[k], p = app[k], z = b[p], x = acc + z * .5; b[p] = x; acc = z - x * .5; app[k] = (p + 1) % b.length; }
    out[i] = acc;
  }
  return out;
}
const rvL = reverb(wet, [2411, 2909, 3413, 3767].map((x) => x * 2), .86, [556, 441, 341]);
const rvR = reverb(wet, [2557, 3001, 3541, 4001].map((x) => x * 2), .86, [521, 421, 313]);
const outL = new Float32Array(N), outR = new Float32Array(N);
let peak = 0;
for (let i = 0; i < N; i++) {
  const t = i / SR, g = gate(t);
  const fadeIn = sstep(0, .08, t), fadeOut = 1 - sstep(58.6, 60, t);
  outL[i] = Math.tanh((dryL[i] + rvL[i] * .55) * 1.1) * g * fadeIn * fadeOut; outR[i] = Math.tanh((dryR[i] + rvR[i] * .55) * 1.1) * g * fadeIn * fadeOut;
  peak = Math.max(peak, Math.abs(outL[i]), Math.abs(outR[i]));
}
const norm = .9 / peak, buf = Buffer.alloc(44 + N * 4);
buf.write("RIFF", 0); buf.writeUInt32LE(36 + N * 4, 4); buf.write("WAVEfmt ", 8); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(2, 22);
buf.writeUInt32LE(SR, 24); buf.writeUInt32LE(SR * 4, 28); buf.writeUInt16LE(4, 32); buf.writeUInt16LE(16, 34); buf.write("data", 36); buf.writeUInt32LE(N * 4, 40);
for (let i = 0; i < N; i++) { buf.writeInt16LE(Math.round(outL[i] * norm * 32767), 44 + i * 4); buf.writeInt16LE(Math.round(outR[i] * norm * 32767), 46 + i * 4); }
fs.mkdirSync(path.join(here, "out"), { recursive: true });
fs.writeFileSync(path.join(here, "out", "korg-trailer.wav"), buf);
console.log("wrote out/korg-trailer.wav  peak(before norm)=" + peak.toFixed(3));
