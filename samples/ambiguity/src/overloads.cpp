#include "overloads.h"

namespace Math {
void Blend(int a) {
  (void)a;
}

void Blend(float a) {
  (void)a;
}
}

namespace Renderer {
void Blend(int a, int b) {
  Math::Blend(a);
  (void)b;
}
}
