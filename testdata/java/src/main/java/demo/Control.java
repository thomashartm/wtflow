package demo;
class Control {
  void control(String[] items) {
    for (String item : items) {
      if (item.equals("skip")) { continue; }
      send(item);
    }
    return;
  }
  void send(String item) { System.out.println(item); }
}
