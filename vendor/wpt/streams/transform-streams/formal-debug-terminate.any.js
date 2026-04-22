// META: global=window,worker,shadowrealm
'use strict';

test(() => {
  new TransformStream({
    start(controller) {
      controller.terminate();
      controller.terminate();
    }
  });
  assert_true(true);
}, 'controller.terminate() should do nothing the second time it is called');