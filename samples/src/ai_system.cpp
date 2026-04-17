#include "ai_system.h"

namespace Game {

AIComponent::AIComponent(GameObject* owner) : m_owner(owner) {}

void AIComponent::UpdateAI(float deltaTime) {
    switch (m_state) {
        case AIState::Idle:    ProcessIdle(deltaTime);    break;
        case AIState::Patrol:  ProcessPatrol(deltaTime);  break;
        case AIState::Chase:   ProcessChase(deltaTime);   break;
        case AIState::Attack:  ProcessAttack(deltaTime);  break;
    }
}

void AIComponent::SetState(AIState state) {
    m_state = state;
}

AIState AIComponent::GetState() const {
    return m_state;
}

void AIComponent::ProcessIdle(float deltaTime) {
    m_owner->Update(deltaTime);
}

void AIComponent::ProcessPatrol(float deltaTime) {
    m_owner->SetPosition(1.0f, 0.0f, 0.0f);
    m_owner->Update(deltaTime);
}

void AIComponent::ProcessChase(float deltaTime) {
    m_owner->Update(deltaTime);
}

void AIComponent::ProcessAttack(float deltaTime) {
    m_owner->Update(deltaTime);
}

void InitializeAISystem(GameWorld* world) {
    auto* obj = world->FindObject("AI_Agent");
    if (obj) {
        obj->SetPosition(0.0f, 0.0f, 0.0f);
    }
}

} // namespace Game
