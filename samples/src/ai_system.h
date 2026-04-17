#pragma once

#include "game_object.h"

namespace Game {

enum class AIState {
    Idle,
    Patrol,
    Chase,
    Attack
};

class AIComponent {
public:
    AIComponent(GameObject* owner);

    void UpdateAI(float deltaTime);
    void SetState(AIState state);
    AIState GetState() const;

private:
    void ProcessIdle(float deltaTime);
    void ProcessPatrol(float deltaTime);
    void ProcessChase(float deltaTime);
    void ProcessAttack(float deltaTime);

    GameObject* m_owner;
    AIState m_state = AIState::Idle;
};

void InitializeAISystem(GameWorld* world);

} // namespace Game
