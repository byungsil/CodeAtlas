#pragma once

#include <string>
#include <vector>

namespace Game {

// Interface with virtual/pure-virtual methods for Phase E/A testing
class IUpdatable {
public:
    virtual void Update(float deltaTime) = 0;  // pure virtual
    virtual void Render() {}                    // virtual with default body
    virtual ~IUpdatable() = default;
};

class GameObject : public IUpdatable {
public:
    GameObject(const std::string& name);
    ~GameObject();

    void Update(float deltaTime) override;
    void Render() override;

    const std::string& GetName() const;
    void SetPosition(float x, float y, float z);

private:
    std::string m_name;
    float m_posX = 0.0f;
    float m_posY = 0.0f;
    float m_posZ = 0.0f;
};

class GameWorld {
public:
    void AddObject(GameObject* obj);
    void RemoveObject(const std::string& name);
    void UpdateAll(float deltaTime);
    void RenderAll();

    GameObject* FindObject(const std::string& name);

private:
    std::vector<GameObject*> m_objects;
};

} // namespace Game
