#include "game_object.h"
#include "ai_system.h"

using namespace Game;

void RunGameLoop(GameWorld& world, float deltaTime) {
    world.UpdateAll(deltaTime);
    world.RenderAll();
}

int main() {
    GameWorld world;

    GameObject player("Player");
    GameObject enemy("AI_Agent");

    world.AddObject(&player);
    world.AddObject(&enemy);

    InitializeAISystem(&world);

    AIComponent ai(&enemy);
    ai.SetState(AIState::Patrol);

    for (int i = 0; i < 100; ++i) {
        ai.UpdateAI(0.016f);
        RunGameLoop(world, 0.016f);
    }

    return 0;
}
