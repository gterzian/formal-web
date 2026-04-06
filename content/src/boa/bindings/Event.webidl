interface Event {
  readonly attribute DOMString type;
  readonly attribute any target;
  readonly attribute any currentTarget;
  readonly attribute unsigned short eventPhase;
  readonly attribute boolean bubbles;
  readonly attribute boolean cancelable;
  readonly attribute boolean defaultPrevented;
  attribute boolean cancelBubble;
  readonly attribute boolean isTrusted;
  readonly attribute unrestricted double timeStamp;
  undefined stopPropagation();
  undefined stopImmediatePropagation();
  undefined preventDefault();
};