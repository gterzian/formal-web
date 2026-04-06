interface Document : Node {
  Element? getElementById(DOMString id);
  Element? querySelector(DOMString selector);
  sequence<Element> querySelectorAll(DOMString selector);
  Element createElement(DOMString localName);
  Node createTextNode(DOMString data);
  readonly attribute Element? body;
  attribute DOMString title;
};