#include "complex_receivers.h"

namespace Game {
void Worker::Update() {}

Worker MakeWorker() {
    return Worker{};
}

void Tick() {
    MakeWorker().Update();
}
}
