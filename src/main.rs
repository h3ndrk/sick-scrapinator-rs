use scrapinator::Lidar;

fn main() {
  let mut lidar = Lidar::connect("192.168.0.2:2112");
  let foo = lidar.poll_data();
  dbg!(foo);
}
