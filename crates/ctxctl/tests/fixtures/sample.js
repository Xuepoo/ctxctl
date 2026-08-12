// Fixture module used by ctxctl integration tests (javascript).

import express from "express";
import { helper } from "./helpers";
const os = require("os");

/** A user entity. */
class User {
  /** Say hello. */
  greet() {
    return "hi";
  }
}

export function formatName(user) {
  return user.name.trim();
}

const MAX_RETRIES = 3;
