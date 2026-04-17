#pragma once

namespace Game {

class Worker {
public:
  void Update();
  void Tick(Worker* other);
};

}
