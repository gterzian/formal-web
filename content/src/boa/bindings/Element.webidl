interface Element : Node {
  readonly attribute DOMString id;
  readonly attribute DOMString tagName;
  attribute DOMString innerHTML;
  DOMString? getAttribute(DOMString name);
  undefined setAttribute(DOMString name, DOMString value);
};