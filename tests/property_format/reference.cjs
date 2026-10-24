const path = require('node:path');
const lib = path.join(process.argv[1], 'lib');
require(path.join(lib, 'parser'));
require(path.join(lib, 'test-receiver'));

// Upstream steps a lookbehind back one UTF-16 unit, into the middle of an astral
// character, so it rejects valid YAML such as `a: 🦀#`.
const chk = Parser.prototype.chk;
Parser.prototype.chk = function (type, expr) {
  if (type !== '<=') return chk.call(this, type, expr);
  const ahead = chk.call(this, '=', expr);
  return name_('chk', function () {
    const pos = this.pos;
    this.pos -= /[\udc00-\udfff]/.test(this.input[pos - 1]) ? 2 : 1;
    const ok = ahead.call(this);
    this.pos = pos;
    return ok;
  }, ahead.trace);
};

// Memoised equivalents of upstream's hottest helpers, which keep the suites inside
// their time budget: process.env reads, string stringify, and receiver lookup.
global.ENV = { ...process.env };
const memo = (cache, key, compute) => {
  let value = cache.get(key);
  if (value === undefined) cache.set(key, (value = compute(key)));
  return value;
};
const stringify = global.stringify;
const strings = new Map();
global.stringify = o => (typeof o === 'string' ? memo(strings, o, stringify) : stringify(o));
const parts = new Map();
const part = name => {
  const char = name.match(/^chr\((.)\)$/);
  return char ? 'x' + hex_char(char[1]) : name.replace(/\(.*/, '');
};
const receivers = new Map();
Parser.prototype.make_receivers = function () {
  let i = this.state.length;
  let n;
  let tail = '';
  while (i > 0 && !(n = this.state[--i].name).includes('_')) {
    tail = '__' + memo(parts, n, part) + tail;
  }
  const prototype = this.receiver.constructor.prototype;
  return memo(receivers, n + tail, name => ({
    try: prototype[`try__${name}`],
    got: prototype[`got__${name}`],
    not: prototype[`not__${name}`],
  }));
};

require('node:readline').createInterface({ input: process.stdin }).on('line', line => {
  const parser = new Parser(new TestReceiver());
  let events = null;
  try {
    parser.parse(JSON.parse(line));
    events = parser.receiver.output();
  } catch (error) {
    if (typeof error !== 'string') throw error;
  }
  process.stdout.write(JSON.stringify(events) + '\n');
});
