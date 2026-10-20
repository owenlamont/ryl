const path = require('node:path');
const root = path.resolve(path.dirname(process.argv[1]), '../lib');
require(path.join(root, 'prelude'));
require(path.join(root, 'parser'));
require(path.join(root, 'test-receiver'));
require('node:readline').createInterface({ input: process.stdin }).on('line', line => {
  const parser = new Parser(new TestReceiver());
  parser.parse(JSON.parse(line));
  const scalar = parser.receiver.event.findLast(event => event.value !== undefined);
  process.stdout.write(JSON.stringify(scalar.value) + '\n');
});
