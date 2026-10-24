// The globals the reference parser takes from ingy-prelude, so it runs without npm.
Object.assign(global, {
  _: {
    isArray: Array.isArray,
    isBoolean: (value) => typeof value === "boolean",
    isFunction: (value) => typeof value === "function",
    isNull: (value) => value === null,
    isNumber: (value) => typeof value === "number",
    isPlainObject: (value) =>
      value !== null && Object.getPrototypeOf(value) === Object.prototype,
    isString: (value) => typeof value === "string",
    join: (array, separator) => array.join(separator),
    keys: Object.keys,
    last: (array) => array.at(-1),
    map: (array, func) => array.map(func),
    repeat: (string, count) => string.repeat(count),
  },
  warn: (...message) => process.stderr.write(`${message.join(" ")}\n`),
  // A rejected input throws a string, as the parser itself does; an Error is a parser bug.
  die: (message) => {
    throw `Died: ${message}`;
  },
  XXX: (...data) => {
    throw new Error(JSON.stringify(data));
  },
});
global.WWW = global.XXX;
