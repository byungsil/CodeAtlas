#include "split_update.h"

namespace Game {

void Worker::Update() {}

void Worker::Tick(Worker* other) {
  this->Update();
  other->Update();
}

}
