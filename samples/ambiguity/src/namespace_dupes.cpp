#include "namespace_dupes.h"

namespace Gameplay {
void Update() {}
}

namespace UI {
void Update() {}
}

namespace AI {
void Controller::Update() {
  Gameplay::Update();
  UI::Update();
}
}
