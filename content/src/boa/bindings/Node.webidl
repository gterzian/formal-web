interface Node : EventTarget {
  attribute DOMString textContent;
  Node appendChild(Node child);
};