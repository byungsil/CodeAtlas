#include "sibling_methods.h"

namespace Game {

void Player::Process() {
  this->Run();
}

void Player::Run() {}

void Enemy::Process() {
  this->Run();
}

void Enemy::Run() {}

}
