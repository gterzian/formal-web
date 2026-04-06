interface UIEvent : Event {
  readonly attribute any view;
  readonly attribute long detail;
};