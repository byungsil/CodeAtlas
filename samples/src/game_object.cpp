#include "game_object.h"
#include <algorithm>

namespace Game {

GameObject::GameObject(const std::string& name) : m_name(name) {}

GameObject::~GameObject() {}

void GameObject::Update(float deltaTime) {
    // placeholder update logic
}

void GameObject::Render() {
    // placeholder render logic
}

const std::string& GameObject::GetName() const {
    return m_name;
}

void GameObject::SetPosition(float x, float y, float z) {
    m_posX = x;
    m_posY = y;
    m_posZ = z;
}

void GameWorld::AddObject(GameObject* obj) {
    m_objects.push_back(obj);
}

void GameWorld::RemoveObject(const std::string& name) {
    m_objects.erase(
        std::remove_if(m_objects.begin(), m_objects.end(),
            [&name](GameObject* obj) { return obj->GetName() == name; }),
        m_objects.end());
}

void GameWorld::UpdateAll(float deltaTime) {
    for (auto* obj : m_objects) {
        obj->Update(deltaTime);
    }
}

void GameWorld::RenderAll() {
    for (auto* obj : m_objects) {
        obj->Render();
    }
}

GameObject* GameWorld::FindObject(const std::string& name) {
    for (auto* obj : m_objects) {
        if (obj->GetName() == name) {
            return obj;
        }
    }
    return nullptr;
}

} // namespace Game
