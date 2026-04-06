interface EventTarget {
  undefined addEventListener(DOMString type, Function? callback, optional any options);
  undefined removeEventListener(DOMString type, Function? callback, optional any options);
  boolean dispatchEvent(Event event);
};