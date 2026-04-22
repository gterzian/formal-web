// META: global=window,worker,shadowrealm
// META: script=../resources/test-utils.js
'use strict';

promise_test(async t => {
  let startCalled = false;
  let startDone = false;
  let transformDone = false;
  let flushDone = false;
  const ts = new TransformStream({
    start() {
      startCalled = true;
      return flushAsyncEvents().then(() => {
        startDone = true;
      });
    },
    transform() {
      return t.step(() => {
        assert_true(startDone,
                    'transform() should not be called until the promise returned from start() has resolved');
        return flushAsyncEvents().then(() => {
          transformDone = true;
        });
      });
    },
    flush() {
      return t.step(() => {
        assert_true(transformDone,
                    'flush() should not be called until the promise returned from transform() has resolved');
        return flushAsyncEvents().then(() => {
          flushDone = true;
        });
      });
    }
  }, undefined, { highWaterMark: 1 });

  assert_true(startCalled, 'start() should be called synchronously');

  const writer = ts.writable.getWriter();
  writer.write('a');
  const closeResult = await Promise.race([
    writer.close().then(() => 'closed', error => `close-rejected:${error}`),
    delay(50).then(() => 'close-timeout')
  ]);
  assert_not_equals(
      closeResult,
      'close-timeout',
      `close=${closeResult}, startDone=${startDone}, transformDone=${transformDone}, flushDone=${flushDone}`);
  assert_equals(closeResult, 'closed');
}, 'TransformStream start, transform, and flush should be strictly ordered');