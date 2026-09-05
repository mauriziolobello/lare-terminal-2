// idle-duck.test.mjs — test per duckStep (node:test)
import { strict as assert } from "node:assert";
import { describe, it }     from "node:test";
import { duckStep, DUCK_SPEED_PX_PER_S } from "./idle-duck.js";

const MAX_X = 500;

describe("duckStep", () => {
  it("si muove a destra quando dir=1", () => {
    const next = duckStep({ x: 0, dir: 1 }, 100, MAX_X);
    assert.ok(next.x > 0, "x deve aumentare");
    assert.equal(next.dir, 1, "direzione invariata finché non rimbalza");
  });

  it("si muove a sinistra quando dir=-1", () => {
    const next = duckStep({ x: 300, dir: -1 }, 100, MAX_X);
    assert.ok(next.x < 300, "x deve diminuire");
    assert.equal(next.dir, -1);
  });

  it("avanza della distanza attesa in 1 secondo", () => {
    const next = duckStep({ x: 0, dir: 1 }, 1000, MAX_X);
    assert.equal(next.x, DUCK_SPEED_PX_PER_S);
  });

  it("rimbalza al bordo destro: dir diventa -1 e x non supera maxX", () => {
    const next = duckStep({ x: MAX_X - 1, dir: 1 }, 500, MAX_X);
    assert.equal(next.x, MAX_X, "deve fermarsi esattamente a maxX");
    assert.equal(next.dir, -1, "deve invertire direzione");
  });

  it("rimbalza al bordo sinistro: dir diventa 1 e x non scende sotto 0", () => {
    const next = duckStep({ x: 1, dir: -1 }, 500, MAX_X);
    assert.equal(next.x, 0, "deve fermarsi esattamente a 0");
    assert.equal(next.dir, 1, "deve invertire direzione");
  });

  it("non fa overshoot: parte già a maxX e rimbalza subito", () => {
    const next = duckStep({ x: MAX_X, dir: 1 }, 100, MAX_X);
    assert.equal(next.x, MAX_X);
    assert.equal(next.dir, -1);
  });

  it("non fa overshoot a sinistra partendo da x=0", () => {
    const next = duckStep({ x: 0, dir: -1 }, 100, MAX_X);
    assert.equal(next.x, 0);
    assert.equal(next.dir, 1);
  });

  it("con dt=0 non si muove", () => {
    const state = { x: 123, dir: 1 };
    const next = duckStep(state, 0, MAX_X);
    assert.equal(next.x, 123);
    assert.equal(next.dir, 1);
  });
});
