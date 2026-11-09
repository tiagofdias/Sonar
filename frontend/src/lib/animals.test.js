import { test } from "node:test";
import assert from "node:assert/strict";
import { ANIMALS, animalOf } from "./animals.js";

test("a chosen animal wins and everyone else gets a stable one", () => {
  assert.equal(animalOf({ _id: "507f1f77bcf86cd799439011", animal: "orca" }), "orca");
  // Desktop's test checks these same two ids.
  assert.equal(animalOf({ _id: "507f1f77bcf86cd799439011" }), ANIMALS[0x99439011 % 16]);
  assert.equal(animalOf({ _id: "0000000000000000000000ff" }), "lobster");
  assert.equal(animalOf({ _id: "507f1f77bcf86cd799439011", animal: "dragon" }), ANIMALS[1]);
  assert.equal(animalOf(null), "whale");
});
