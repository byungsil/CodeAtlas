import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import request from "supertest";
import { createApp } from "../app";
import { JsonStore } from "../storage/json-store";
import { mcpCall } from "./mcp-test-helpers";
import { Symbol } from "../models/symbol";
import { Call } from "../models/call";
import { FileRecord } from "../models/file-record";
import { PropagationEventRecord, ReferenceRecord } from "../models/responses";
import { deriveLanguageFromPath } from "../language";

function withLanguage<T extends { filePath: string }>(record: T): T & { language: ReturnType<typeof deriveLanguageFromPath> } {
  return {
    ...record,
    language: deriveLanguageFromPath(record.filePath),
  };
}

function withFileLanguage<T extends { path: string }>(record: T): T & { language: ReturnType<typeof deriveLanguageFromPath> } {
  return {
    ...record,
    language: deriveLanguageFromPath(record.path),
  };
}

function fixtureSymbols(): Symbol[] {
  return [
    withLanguage({
      id: "Game::Investigation",
      name: "Investigation",
      qualifiedName: "Game::Investigation",
      type: "namespace",
      filePath: "samples/investigation/src/workflow.h",
      line: 1,
      endLine: 24,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotRequest",
      name: "ShotRequest",
      qualifiedName: "Game::Investigation::ShotRequest",
      type: "struct",
      filePath: "samples/investigation/src/workflow.h",
      line: 4,
      endLine: 7,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::EventHint",
      name: "EventHint",
      qualifiedName: "Game::Investigation::EventHint",
      type: "struct",
      filePath: "samples/investigation/src/workflow.h",
      line: 9,
      endLine: 11,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::EventEnvelope",
      name: "EventEnvelope",
      qualifiedName: "Game::Investigation::EventEnvelope",
      type: "struct",
      filePath: "samples/investigation/src/workflow.h",
      line: 13,
      endLine: 15,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::NestedEnvelope",
      name: "NestedEnvelope",
      qualifiedName: "Game::Investigation::NestedEnvelope",
      type: "struct",
      filePath: "samples/investigation/src/workflow.h",
      line: 17,
      endLine: 19,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotController",
      name: "ShotController",
      qualifiedName: "Game::Investigation::ShotController",
      type: "class",
      filePath: "samples/investigation/src/workflow.h",
      line: 9,
      endLine: 16,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::HintController",
      name: "HintController",
      qualifiedName: "Game::Investigation::HintController",
      type: "class",
      filePath: "samples/investigation/src/workflow.h",
      line: 23,
      endLine: 30,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ConstructedHintController",
      name: "ConstructedHintController",
      qualifiedName: "Game::Investigation::ConstructedHintController",
      type: "class",
      filePath: "samples/investigation/src/workflow.h",
      line: 31,
      endLine: 38,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::NestedConstructedHintController",
      name: "NestedConstructedHintController",
      qualifiedName: "Game::Investigation::NestedConstructedHintController",
      type: "class",
      filePath: "samples/investigation/src/workflow.h",
      line: 48,
      endLine: 55,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RelayFieldController",
      name: "RelayFieldController",
      qualifiedName: "Game::Investigation::RelayFieldController",
      type: "class",
      filePath: "samples/investigation/src/workflow.h",
      line: 57,
      endLine: 64,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::HintController::hintedPower",
      name: "hintedPower",
      qualifiedName: "Game::Investigation::HintController::hintedPower",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 28,
      endLine: 28,
      parentId: "Game::Investigation::HintController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ConstructedHintController::seededPower",
      name: "seededPower",
      qualifiedName: "Game::Investigation::ConstructedHintController::seededPower",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 36,
      endLine: 36,
      parentId: "Game::Investigation::ConstructedHintController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::NestedConstructedHintController::seededPower",
      name: "seededPower",
      qualifiedName: "Game::Investigation::NestedConstructedHintController::seededPower",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 53,
      endLine: 53,
      parentId: "Game::Investigation::NestedConstructedHintController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RelayFieldController::storedPower",
      name: "storedPower",
      qualifiedName: "Game::Investigation::RelayFieldController::storedPower",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 62,
      endLine: 62,
      parentId: "Game::Investigation::RelayFieldController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotController::queuedPower",
      name: "queuedPower",
      qualifiedName: "Game::Investigation::ShotController::queuedPower",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 14,
      endLine: 14,
      parentId: "Game::Investigation::ShotController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotController::queuedArmed",
      name: "queuedArmed",
      qualifiedName: "Game::Investigation::ShotController::queuedArmed",
      type: "variable",
      filePath: "samples/investigation/src/workflow.h",
      line: 15,
      endLine: 15,
      parentId: "Game::Investigation::ShotController",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ReadInputPower",
      name: "ReadInputPower",
      qualifiedName: "Game::Investigation::ReadInputPower",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 6,
      endLine: 8,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ReadInputArmed",
      name: "ReadInputArmed",
      qualifiedName: "Game::Investigation::ReadInputArmed",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 10,
      endLine: 12,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::BuildShotRequest",
      name: "BuildShotRequest",
      qualifiedName: "Game::Investigation::BuildShotRequest",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 14,
      endLine: 17,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "ShotRequest BuildShotRequest(int power, bool armed)",
      parameterCount: 2,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::MakeHint",
      name: "MakeHint",
      qualifiedName: "Game::Investigation::MakeHint",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 19,
      endLine: 22,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "EventHint MakeHint(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::MakeEnvelope",
      name: "MakeEnvelope",
      qualifiedName: "Game::Investigation::MakeEnvelope",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 24,
      endLine: 27,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "EventEnvelope MakeEnvelope(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::MakeNestedEnvelope",
      name: "MakeNestedEnvelope",
      qualifiedName: "Game::Investigation::MakeNestedEnvelope",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 29,
      endLine: 32,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "NestedEnvelope MakeNestedEnvelope(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ExtractHintPower",
      name: "ExtractHintPower",
      qualifiedName: "Game::Investigation::ExtractHintPower",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 29,
      endLine: 31,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "int ExtractHintPower(const EventHint& hint)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotController::LoadRequest",
      name: "LoadRequest",
      qualifiedName: "Game::Investigation::ShotController::LoadRequest",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 24,
      endLine: 27,
      parentId: "Game::Investigation::ShotController",
      signature: "void LoadRequest(const ShotRequest& request)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::LaunchShot",
      name: "LaunchShot",
      qualifiedName: "Game::Investigation::LaunchShot",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 29,
      endLine: 31,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void LaunchShot(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ShotController::LaunchIfReady",
      name: "LaunchIfReady",
      qualifiedName: "Game::Investigation::ShotController::LaunchIfReady",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 33,
      endLine: 38,
      parentId: "Game::Investigation::ShotController",
      signature: "void LaunchIfReady()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::HintController::ApplyHint",
      name: "ApplyHint",
      qualifiedName: "Game::Investigation::HintController::ApplyHint",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 40,
      endLine: 42,
      parentId: "Game::Investigation::HintController",
      signature: "void ApplyHint(const EventHint& hint)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::LaunchHint",
      name: "LaunchHint",
      qualifiedName: "Game::Investigation::LaunchHint",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 44,
      endLine: 46,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void LaunchHint(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::EmitRelayHint",
      name: "EmitRelayHint",
      qualifiedName: "Game::Investigation::EmitRelayHint",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 50,
      endLine: 52,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void EmitRelayHint(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::EmitForwardedPower",
      name: "EmitForwardedPower",
      qualifiedName: "Game::Investigation::EmitForwardedPower",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 54,
      endLine: 56,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void EmitForwardedPower(int power)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::HintController::EmitHint",
      name: "EmitHint",
      qualifiedName: "Game::Investigation::HintController::EmitHint",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 48,
      endLine: 51,
      parentId: "Game::Investigation::HintController",
      signature: "void EmitHint()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RelayFieldController::ApplyHint",
      name: "ApplyHint",
      qualifiedName: "Game::Investigation::RelayFieldController::ApplyHint",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 68,
      endLine: 70,
      parentId: "Game::Investigation::RelayFieldController",
      signature: "void ApplyHint(const EventHint& hint)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RelayFieldController::EmitStored",
      name: "EmitStored",
      qualifiedName: "Game::Investigation::RelayFieldController::EmitStored",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      endLine: 74,
      parentId: "Game::Investigation::RelayFieldController",
      signature: "void EmitStored()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ConstructedHintController::ConstructedHintController",
      name: "ConstructedHintController",
      qualifiedName: "Game::Investigation::ConstructedHintController::ConstructedHintController",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 53,
      endLine: 54,
      parentId: "Game::Investigation::ConstructedHintController",
      signature: "ConstructedHintController(int initialPower)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::ConstructedHintController::EmitConstructed",
      name: "EmitConstructed",
      qualifiedName: "Game::Investigation::ConstructedHintController::EmitConstructed",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 56,
      endLine: 59,
      parentId: "Game::Investigation::ConstructedHintController",
      signature: "void EmitConstructed()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController",
      name: "NestedConstructedHintController",
      qualifiedName: "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      endLine: 73,
      parentId: "Game::Investigation::NestedConstructedHintController",
      signature: "NestedConstructedHintController(int initialPower)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed",
      name: "EmitNestedConstructed",
      qualifiedName: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed",
      type: "method",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 75,
      endLine: 78,
      parentId: "Game::Investigation::NestedConstructedHintController",
      signature: "void EmitNestedConstructed()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::QueueShot",
      name: "QueueShot",
      qualifiedName: "Game::Investigation::QueueShot",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 53,
      endLine: 59,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void QueueShot(ShotController& controller)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunConstructedHintWorkflow",
      name: "RunConstructedHintWorkflow",
      qualifiedName: "Game::Investigation::RunConstructedHintWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 69,
      endLine: 72,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunConstructedHintWorkflow()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunNestedConstructedHintWorkflow",
      name: "RunNestedConstructedHintWorkflow",
      qualifiedName: "Game::Investigation::RunNestedConstructedHintWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 96,
      endLine: 99,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunNestedConstructedHintWorkflow()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunRelayFieldWorkflow",
      name: "RunRelayFieldWorkflow",
      qualifiedName: "Game::Investigation::RunRelayFieldWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 97,
      endLine: 101,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunRelayFieldWorkflow(RelayFieldController& controller)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunHintWorkflow",
      name: "RunHintWorkflow",
      qualifiedName: "Game::Investigation::RunHintWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 61,
      endLine: 65,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunHintWorkflow(HintController& controller)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunNestedHintWorkflow",
      name: "RunNestedHintWorkflow",
      qualifiedName: "Game::Investigation::RunNestedHintWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 71,
      endLine: 75,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunNestedHintWorkflow(HintController& controller)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunNestedRelayWorkflow",
      name: "RunNestedRelayWorkflow",
      qualifiedName: "Game::Investigation::RunNestedRelayWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 100,
      endLine: 104,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunNestedRelayWorkflow()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      name: "RunNestedRelayToForwarderWorkflow",
      qualifiedName: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      type: "function",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 109,
      endLine: 113,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void RunNestedRelayToForwarderWorkflow()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Investigation::PendingState",
      name: "PendingState",
      qualifiedName: "Game::Investigation::PendingState",
      type: "struct",
      filePath: "samples/investigation/src/partial_flow.h",
      line: 4,
      endLine: 6,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::PendingState::armed",
      name: "armed",
      qualifiedName: "Game::Investigation::PendingState::armed",
      type: "variable",
      filePath: "samples/investigation/src/partial_flow.h",
      line: 5,
      endLine: 5,
      parentId: "Game::Investigation::PendingState",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::CopyArmedFlag",
      name: "CopyArmedFlag",
      qualifiedName: "Game::Investigation::CopyArmedFlag",
      type: "function",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 6,
      endLine: 10,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void CopyArmedFlag(PendingState* state, bool value)",
      parameterCount: 2,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::ConsumeArmedFlag",
      name: "ConsumeArmedFlag",
      qualifiedName: "Game::Investigation::ConsumeArmedFlag",
      type: "function",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 12,
      endLine: 18,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "bool ConsumeArmedFlag(PendingState* state)",
      parameterCount: 1,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::HandleFallback",
      name: "HandleFallback",
      qualifiedName: "Game::Investigation::HandleFallback",
      type: "function",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 20,
      endLine: 25,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void HandleFallback(PendingState* state, bool value)",
      parameterCount: 2,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::WeakProbe",
      name: "WeakProbe",
      qualifiedName: "Game::Investigation::WeakProbe",
      type: "function",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 27,
      endLine: 29,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void WeakProbe()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Investigation::WeakHelper",
      name: "WeakHelper",
      qualifiedName: "Game::Investigation::WeakHelper",
      type: "function",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 31,
      endLine: 33,
      scopeQualifiedName: "Game::Investigation",
      scopeKind: "namespace",
      signature: "void WeakHelper()",
      parameterCount: 0,
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
      parseFragility: "elevated",
      macroSensitivity: "low",
      includeHeaviness: "light",
    }),
    withLanguage({
      id: "Game::Runtime::UpdateShot",
      name: "UpdateShot",
      qualifiedName: "Game::Runtime::UpdateShot",
      type: "function",
      filePath: "samples/investigation/runtime/update_shot.cpp",
      line: 4,
      endLine: 4,
      scopeQualifiedName: "Game::Runtime",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Runtime::TickRuntimeShot",
      name: "TickRuntimeShot",
      qualifiedName: "Game::Runtime::TickRuntimeShot",
      type: "function",
      filePath: "samples/investigation/runtime/update_shot.cpp",
      line: 6,
      endLine: 8,
      scopeQualifiedName: "Game::Runtime",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Runtime::ShotPanel",
      name: "ShotPanel",
      qualifiedName: "Game::Runtime::ShotPanel",
      type: "class",
      filePath: "samples/investigation/runtime/update_shot.cpp",
      line: 10,
      endLine: 14,
      scopeQualifiedName: "Game::Runtime",
      scopeKind: "namespace",
      module: "gameplay",
      subsystem: "runtime",
      projectArea: "investigation",
      artifactKind: "runtime",
    }),
    withLanguage({
      id: "Game::Editor::UpdateShot",
      name: "UpdateShot",
      qualifiedName: "Game::Editor::UpdateShot",
      type: "function",
      filePath: "samples/investigation/editor/update_shot.cpp",
      line: 4,
      endLine: 4,
      scopeQualifiedName: "Game::Editor",
      scopeKind: "namespace",
      module: "editor",
      subsystem: "editor",
      projectArea: "investigation",
      artifactKind: "editor",
    }),
    withLanguage({
      id: "Game::Editor::ShotPanel",
      name: "ShotPanel",
      qualifiedName: "Game::Editor::ShotPanel",
      type: "class",
      filePath: "samples/investigation/editor/update_shot.cpp",
      line: 10,
      endLine: 16,
      scopeQualifiedName: "Game::Editor",
      scopeKind: "namespace",
      module: "editor",
      subsystem: "editor",
      projectArea: "investigation",
      artifactKind: "editor",
    }),
    withLanguage({
      id: "Game::Editor::RefreshShotPreview",
      name: "RefreshShotPreview",
      qualifiedName: "Game::Editor::RefreshShotPreview",
      type: "function",
      filePath: "samples/investigation/editor/update_shot.cpp",
      line: 6,
      endLine: 8,
      scopeQualifiedName: "Game::Editor",
      scopeKind: "namespace",
      module: "editor",
      subsystem: "editor",
      projectArea: "investigation",
      artifactKind: "editor",
    }),
  ];
}

function fixtureCalls(): Call[] {
  return [
    {
      callerId: "Game::Investigation::QueueShot",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 54,
    },
    {
      callerId: "Game::Investigation::QueueShot",
      calleeId: "Game::Investigation::ReadInputArmed",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 55,
    },
    {
      callerId: "Game::Investigation::QueueShot",
      calleeId: "Game::Investigation::BuildShotRequest",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 56,
    },
    {
      callerId: "Game::Investigation::QueueShot",
      calleeId: "Game::Investigation::ShotController::LoadRequest",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 57,
    },
    {
      callerId: "Game::Investigation::QueueShot",
      calleeId: "Game::Investigation::ShotController::LaunchIfReady",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 58,
    },
    {
      callerId: "Game::Investigation::ShotController::LaunchIfReady",
      calleeId: "Game::Investigation::LaunchShot",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 36,
    },
    {
      callerId: "Game::Investigation::RunHintWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 62,
    },
    {
      callerId: "Game::Investigation::RunHintWorkflow",
      calleeId: "Game::Investigation::MakeHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 62,
    },
    {
      callerId: "Game::Investigation::RunHintWorkflow",
      calleeId: "Game::Investigation::HintController::ApplyHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 63,
    },
    {
      callerId: "Game::Investigation::RunHintWorkflow",
      calleeId: "Game::Investigation::HintController::EmitHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 64,
    },
    {
      callerId: "Game::Investigation::HintController::EmitHint",
      calleeId: "Game::Investigation::LaunchHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 50,
    },
    {
      callerId: "Game::Investigation::MakeEnvelope",
      calleeId: "Game::Investigation::MakeHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 25,
    },
    {
      callerId: "Game::Investigation::MakeNestedEnvelope",
      calleeId: "Game::Investigation::MakeEnvelope",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 30,
    },
    {
      callerId: "Game::Investigation::RunNestedHintWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
    },
    {
      callerId: "Game::Investigation::RunNestedHintWorkflow",
      calleeId: "Game::Investigation::MakeNestedEnvelope",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
    },
    {
      callerId: "Game::Investigation::RunNestedHintWorkflow",
      calleeId: "Game::Investigation::HintController::ApplyHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 73,
    },
    {
      callerId: "Game::Investigation::RunNestedHintWorkflow",
      calleeId: "Game::Investigation::HintController::EmitHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 74,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 101,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayWorkflow",
      calleeId: "Game::Investigation::MakeNestedEnvelope",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 101,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayWorkflow",
      calleeId: "Game::Investigation::ExtractHintPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 102,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayWorkflow",
      calleeId: "Game::Investigation::EmitRelayHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 103,
    },
    {
      callerId: "Game::Investigation::EmitRelayHint",
      calleeId: "Game::Investigation::LaunchHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 51,
    },
    {
      callerId: "Game::Investigation::EmitForwardedPower",
      calleeId: "Game::Investigation::EmitRelayHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 55,
    },
    {
      callerId: "Game::Investigation::RelayFieldController::EmitStored",
      calleeId: "Game::Investigation::EmitRelayHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 73,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 110,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      calleeId: "Game::Investigation::MakeNestedEnvelope",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 110,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      calleeId: "Game::Investigation::ExtractHintPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 111,
    },
    {
      callerId: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      calleeId: "Game::Investigation::EmitForwardedPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 112,
    },
    {
      callerId: "Game::Investigation::RunConstructedHintWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 70,
    },
    {
      callerId: "Game::Investigation::RunConstructedHintWorkflow",
      calleeId: "Game::Investigation::ConstructedHintController::ConstructedHintController",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 70,
    },
    {
      callerId: "Game::Investigation::RunConstructedHintWorkflow",
      calleeId: "Game::Investigation::ConstructedHintController::EmitConstructed",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 71,
    },
    {
      callerId: "Game::Investigation::ConstructedHintController::EmitConstructed",
      calleeId: "Game::Investigation::LaunchHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 58,
    },
    {
      callerId: "Game::Investigation::RunNestedConstructedHintWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 97,
    },
    {
      callerId: "Game::Investigation::RunNestedConstructedHintWorkflow",
      calleeId: "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 97,
    },
    {
      callerId: "Game::Investigation::RunNestedConstructedHintWorkflow",
      calleeId: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 98,
    },
    {
      callerId: "Game::Investigation::RunRelayFieldWorkflow",
      calleeId: "Game::Investigation::ReadInputPower",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 98,
    },
    {
      callerId: "Game::Investigation::RunRelayFieldWorkflow",
      calleeId: "Game::Investigation::MakeHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 98,
    },
    {
      callerId: "Game::Investigation::RunRelayFieldWorkflow",
      calleeId: "Game::Investigation::RelayFieldController::ApplyHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 99,
    },
    {
      callerId: "Game::Investigation::RunRelayFieldWorkflow",
      calleeId: "Game::Investigation::RelayFieldController::EmitStored",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 100,
    },
    {
      callerId: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed",
      calleeId: "Game::Investigation::LaunchHint",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 77,
    },
    {
      callerId: "Game::Investigation::HandleFallback",
      calleeId: "Game::Investigation::CopyArmedFlag",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 21,
    },
    {
      callerId: "Game::Investigation::HandleFallback",
      calleeId: "Game::Investigation::ConsumeArmedFlag",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 22,
    },
    {
      callerId: "Game::Runtime::TickRuntimeShot",
      calleeId: "Game::Runtime::UpdateShot",
      filePath: "samples/investigation/runtime/update_shot.cpp",
      line: 7,
    },
    {
      callerId: "Game::Editor::RefreshShotPreview",
      calleeId: "Game::Editor::UpdateShot",
      filePath: "samples/investigation/editor/update_shot.cpp",
      line: 7,
    },
    {
      callerId: "Game::Investigation::WeakProbe",
      calleeId: "Game::Investigation::WeakHelper",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 28,
    },
  ];
}

function fixtureReferences(): ReferenceRecord[] {
  return [
    {
      sourceSymbolId: "Game::Investigation::BuildShotRequest",
      targetSymbolId: "Game::Investigation::ShotRequest",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 14,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::QueueShot",
      targetSymbolId: "Game::Investigation::ShotController",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 53,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::MakeHint",
      targetSymbolId: "Game::Investigation::EventHint",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 19,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::MakeEnvelope",
      targetSymbolId: "Game::Investigation::EventEnvelope",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 24,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::MakeNestedEnvelope",
      targetSymbolId: "Game::Investigation::NestedEnvelope",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 29,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunHintWorkflow",
      targetSymbolId: "Game::Investigation::HintController",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 61,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunNestedHintWorkflow",
      targetSymbolId: "Game::Investigation::HintController",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 71,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunNestedRelayWorkflow",
      targetSymbolId: "Game::Investigation::NestedEnvelope",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 100,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
      targetSymbolId: "Game::Investigation::NestedEnvelope",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 109,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunConstructedHintWorkflow",
      targetSymbolId: "Game::Investigation::ConstructedHintController",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 69,
      confidence: "high",
    },
    {
      sourceSymbolId: "Game::Investigation::RunNestedConstructedHintWorkflow",
      targetSymbolId: "Game::Investigation::NestedConstructedHintController",
      category: "typeUsage",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 96,
      confidence: "high",
    },
  ];
}

function fixturePropagationEvents(): PropagationEventRecord[] {
  return [
    {
      ownerSymbolId: "Game::Investigation::ShotController::LoadRequest",
      sourceAnchor: {
        anchorId: "Game::Investigation::ShotController::LoadRequest::request.power",
        anchorKind: "expression",
        expressionText: "request.power",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::ShotController::queuedPower",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 25,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ShotController::LaunchIfReady",
      sourceAnchor: {
        symbolId: "Game::Investigation::ShotController::queuedPower",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ShotController::LaunchIfReady::launchPower",
        anchorKind: "localVariable",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 35,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ShotController::LoadRequest",
      sourceAnchor: {
        anchorId: "Game::Investigation::ShotController::LoadRequest::request.armed",
        anchorKind: "expression",
        expressionText: "request.armed",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::ShotController::queuedArmed",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 26,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ShotController::LaunchIfReady",
      sourceAnchor: {
        symbolId: "Game::Investigation::ShotController::queuedArmed",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ShotController::LaunchIfReady::branch.armed",
        anchorKind: "expression",
        expressionText: "this->queuedArmed",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 34,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunHintWorkflow::local:inputPower",
        anchorKind: "localVariable",
        expressionText: "inputPower",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::MakeHint::param:power",
        symbolId: "Game::Investigation::MakeHint",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 59,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::MakeHint::return",
        symbolId: "Game::Investigation::MakeHint",
        anchorKind: "returnValue",
        expressionText: "hint",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunHintWorkflow::local:hint",
        anchorKind: "localVariable",
        expressionText: "hint",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 59,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::HintController::ApplyHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunHintWorkflow::local:hint",
        anchorKind: "localVariable",
        expressionText: "hint",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::HintController::ApplyHint::param:hint",
        symbolId: "Game::Investigation::HintController::ApplyHint",
        anchorKind: "parameter",
        expressionText: "hint",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 60,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::HintController::ApplyHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::HintController::ApplyHint::hint.power",
        anchorKind: "expression",
        expressionText: "hint.power",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::HintController::hintedPower",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 41,
      confidence: "partial",
      risks: ["receiverAmbiguity"],
    },
    {
      ownerSymbolId: "Game::Investigation::HintController::EmitHint",
      sourceAnchor: {
        symbolId: "Game::Investigation::HintController::hintedPower",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::HintController::EmitHint::launchPower",
        anchorKind: "localVariable",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 49,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::RelayFieldController::ApplyHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RelayFieldController::ApplyHint::hint.power",
        anchorKind: "expression",
        expressionText: "hint.power",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::RelayFieldController::storedPower",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 68,
      confidence: "partial",
      risks: ["receiverAmbiguity"],
    },
    {
      ownerSymbolId: "Game::Investigation::RelayFieldController::EmitStored",
      sourceAnchor: {
        symbolId: "Game::Investigation::RelayFieldController::storedPower",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RelayFieldController::EmitStored::relayPower",
        anchorKind: "localVariable",
        expressionText: "relayPower",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::EmitRelayHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RelayFieldController::EmitStored::relayPower",
        anchorKind: "localVariable",
        expressionText: "relayPower",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::EmitRelayHint::param:power",
        symbolId: "Game::Investigation::EmitRelayHint",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayWorkflow::local:inputPower",
        anchorKind: "localVariable",
        expressionText: "inputPower",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::param:power",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 101,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::return",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "returnValue",
        expressionText: "nested",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayWorkflow::local:nested",
        anchorKind: "localVariable",
        expressionText: "nested",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 101,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ExtractHintPower",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayWorkflow::local:nested.envelope.hint",
        anchorKind: "localVariable",
        expressionText: "nested.envelope.hint",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ExtractHintPower::param:hint",
        symbolId: "Game::Investigation::ExtractHintPower",
        anchorKind: "parameter",
        expressionText: "hint",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 102,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ExtractHintPower",
      sourceAnchor: {
        anchorId: "Game::Investigation::ExtractHintPower::return",
        symbolId: "Game::Investigation::ExtractHintPower",
        anchorKind: "returnValue",
        expressionText: "power",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayWorkflow::local:power",
        anchorKind: "localVariable",
        expressionText: "power",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 102,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::EmitRelayHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayWorkflow::local:power",
        anchorKind: "localVariable",
        expressionText: "power",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::EmitRelayHint::param:power",
        symbolId: "Game::Investigation::EmitRelayHint",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 103,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayToForwarderWorkflow::local:inputPower",
        anchorKind: "localVariable",
        expressionText: "inputPower",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::param:power",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 110,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::return",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "returnValue",
        expressionText: "nested",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayToForwarderWorkflow::local:nested",
        anchorKind: "localVariable",
        expressionText: "nested",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 110,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ExtractHintPower",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayToForwarderWorkflow::local:nested.envelope.hint",
        anchorKind: "localVariable",
        expressionText: "nested.envelope.hint",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ExtractHintPower::param:hint",
        symbolId: "Game::Investigation::ExtractHintPower",
        anchorKind: "parameter",
        expressionText: "hint",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 111,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ExtractHintPower",
      sourceAnchor: {
        anchorId: "Game::Investigation::ExtractHintPower::return",
        symbolId: "Game::Investigation::ExtractHintPower",
        anchorKind: "returnValue",
        expressionText: "power",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayToForwarderWorkflow::local:power",
        anchorKind: "localVariable",
        expressionText: "power",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 111,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::EmitForwardedPower",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedRelayToForwarderWorkflow::local:power",
        anchorKind: "localVariable",
        expressionText: "power",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::EmitForwardedPower::param:power",
        symbolId: "Game::Investigation::EmitForwardedPower",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 112,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedHintWorkflow::local:inputPower",
        anchorKind: "localVariable",
        expressionText: "inputPower",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::param:power",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "parameter",
        expressionText: "power",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::MakeNestedEnvelope",
      sourceAnchor: {
        anchorId: "Game::Investigation::MakeNestedEnvelope::return",
        symbolId: "Game::Investigation::MakeNestedEnvelope",
        anchorKind: "returnValue",
        expressionText: "nested",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::RunNestedHintWorkflow::local:nested",
        anchorKind: "localVariable",
        expressionText: "nested",
      },
      propagationKind: "returnValue",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 72,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::HintController::ApplyHint",
      sourceAnchor: {
        anchorId: "Game::Investigation::RunNestedHintWorkflow::local:nested.envelope.hint",
        anchorKind: "localVariable",
        expressionText: "nested.envelope.hint",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::HintController::ApplyHint::param:hint",
        symbolId: "Game::Investigation::HintController::ApplyHint",
        anchorKind: "parameter",
        expressionText: "hint",
      },
      propagationKind: "argumentToParameter",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 73,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::ConstructedHintController::ConstructedHintController",
      sourceAnchor: {
        anchorId: "Game::Investigation::ConstructedHintController::ConstructedHintController::MakeHint(initialPower).power",
        anchorKind: "expression",
        expressionText: "MakeHint(initialPower).power",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::ConstructedHintController::seededPower",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 54,
      confidence: "partial",
      risks: ["receiverAmbiguity"],
    },
    {
      ownerSymbolId: "Game::Investigation::ConstructedHintController::EmitConstructed",
      sourceAnchor: {
        symbolId: "Game::Investigation::ConstructedHintController::seededPower",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ConstructedHintController::EmitConstructed::launchPower",
        anchorKind: "localVariable",
        expressionText: "launchPower",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 57,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController",
      sourceAnchor: {
        anchorId: "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController::MakeNestedEnvelope(initialPower).envelope.hint.power",
        anchorKind: "expression",
        expressionText: "MakeNestedEnvelope(initialPower).envelope.hint.power",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::NestedConstructedHintController::seededPower",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 73,
      confidence: "partial",
      risks: ["receiverAmbiguity"],
    },
    {
      ownerSymbolId: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed",
      sourceAnchor: {
        symbolId: "Game::Investigation::NestedConstructedHintController::seededPower",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed::launchPower",
        anchorKind: "localVariable",
        expressionText: "launchPower",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/workflow.cpp",
      line: 76,
      confidence: "high",
      risks: [],
    },
    {
      ownerSymbolId: "Game::Investigation::CopyArmedFlag",
      sourceAnchor: {
        anchorId: "Game::Investigation::CopyArmedFlag::value",
        anchorKind: "parameter",
      },
      targetAnchor: {
        symbolId: "Game::Investigation::PendingState::armed",
        anchorKind: "field",
      },
      propagationKind: "fieldWrite",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 8,
      confidence: "partial",
      risks: ["pointerHeavyFlow"],
    },
    {
      ownerSymbolId: "Game::Investigation::ConsumeArmedFlag",
      sourceAnchor: {
        symbolId: "Game::Investigation::PendingState::armed",
        anchorKind: "field",
      },
      targetAnchor: {
        anchorId: "Game::Investigation::ConsumeArmedFlag::return",
        anchorKind: "returnValue",
      },
      propagationKind: "fieldRead",
      filePath: "samples/investigation/src/partial_flow.cpp",
      line: 17,
      confidence: "partial",
      risks: ["pointerHeavyFlow"],
    },
  ];
}

function fixtureFiles(): FileRecord[] {
  return [
    withFileLanguage({
      path: "samples/investigation/src/workflow.h",
      contentHash: "fixture-investigation-workflow-h",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 8,
    }),
    withFileLanguage({
      path: "samples/investigation/src/workflow.cpp",
      contentHash: "fixture-investigation-workflow-cpp",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 12,
    }),
    withFileLanguage({
      path: "samples/investigation/src/partial_flow.h",
      contentHash: "fixture-investigation-partial-h",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 2,
    }),
    withFileLanguage({
      path: "samples/investigation/src/partial_flow.cpp",
      contentHash: "fixture-investigation-partial-cpp",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 4,
    }),
    withFileLanguage({
      path: "samples/investigation/runtime/update_shot.cpp",
      contentHash: "fixture-investigation-runtime-update-shot",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 2,
    }),
    withFileLanguage({
      path: "samples/investigation/editor/update_shot.cpp",
      contentHash: "fixture-investigation-editor-update-shot",
      lastIndexed: "2026-04-19T00:00:00.000Z",
      symbolCount: 2,
    }),
  ];
}

function writeFixtureJsonStore(): { dir: string; store: JsonStore } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeatlas-investigation-json-"));
  const store = new JsonStore(dir);
  store.save({
    symbols: fixtureSymbols(),
    calls: fixtureCalls(),
    references: fixtureReferences(),
    propagationEvents: fixturePropagationEvents(),
    files: fixtureFiles(),
  });
  return { dir, store };
}

const INIT = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "investigation-fixture-test", version: "1.0" } } };
const INITIALIZED = { jsonrpc: "2.0", method: "notifications/initialized" };

describe("investigation fixture storage and workflow prerequisites", () => {
  it("preserves duplicate short-name startup candidates across runtime and editor paths", () => {
    const { dir, store } = writeFixtureJsonStore();
    try {
      const updateShot = store.getSymbolsByName("UpdateShot");
      expect(updateShot).toHaveLength(2);
      expect(updateShot.map((symbol) => symbol.qualifiedName).sort()).toEqual([
        "Game::Editor::UpdateShot",
        "Game::Runtime::UpdateShot",
      ]);
      expect(updateShot.map((symbol) => symbol.artifactKind).sort()).toEqual(["editor", "runtime"]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("supports a bounded call path from queueing input to launch action", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 2,
          method: "tools/call",
          params: {
            name: "trace_call_path",
            arguments: {
              sourceQualifiedName: "Game::Investigation::QueueShot",
              targetQualifiedName: "Game::Investigation::LaunchShot",
              maxDepth: 3,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 2).result.content[0].text);
      expect(payload.pathFound).toBe(true);
      expect(payload.steps).toHaveLength(2);
      expect(payload.steps[0].callerQualifiedName).toBe("Game::Investigation::QueueShot");
      expect(payload.steps[0].calleeQualifiedName).toBe("Game::Investigation::ShotController::LaunchIfReady");
      expect(payload.steps[1].callerQualifiedName).toBe("Game::Investigation::ShotController::LaunchIfReady");
      expect(payload.steps[1].calleeQualifiedName).toBe("Game::Investigation::LaunchShot");
      expect(payload.truncated).toBe(false);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces strong and partial field-centric propagation needed for MS10 workflow stitching", async () => {
    const { dir } = writeFixtureJsonStore();
    const app = createApp(new JsonStore(dir));
    try {
      const strong = await request(app)
        .get("/symbol-propagation")
        .query({ qualifiedName: "Game::Investigation::ShotController::queuedPower", limit: 10 })
        .expect(200);
      expect(strong.body.lookupMode).toBe("exact");
      expect(strong.body.propagationConfidence).toBe("high");
      expect(strong.body.incoming).toHaveLength(1);
      expect(strong.body.outgoing).toHaveLength(1);
      expect(strong.body.riskMarkers).toEqual([]);
      expect(strong.body.incoming[0].propagationKind).toBe("fieldWrite");
      expect(strong.body.outgoing[0].propagationKind).toBe("fieldRead");

      const partial = await request(app)
        .get("/symbol-propagation")
        .query({ qualifiedName: "Game::Investigation::PendingState::armed", limit: 10 })
        .expect(200);
      expect(partial.body.lookupMode).toBe("exact");
      expect(partial.body.propagationConfidence).toBe("partial");
      expect(partial.body.incoming).toHaveLength(1);
      expect(partial.body.outgoing).toHaveLength(1);
      expect(partial.body.riskMarkers).toContain("pointerHeavyFlow");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("keeps heuristic UpdateShot lookup ambiguous without explicit context hints", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 3,
          method: "tools/call",
          params: { name: "lookup_function", arguments: { name: "UpdateShot" } },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 3).result.content[0].text);
      expect(payload.lookupMode).toBe("heuristic");
      expect(payload.confidence).toBe("ambiguous");
      expect(payload.ambiguity).toEqual({ candidateCount: 2 });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("uses artifact and path context to rank ambiguous UpdateShot candidates", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const runtimeResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 31,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              artifactKind: "runtime",
            },
          },
        },
      ], dir);
      const runtimePayload = JSON.parse(runtimeResponses.find((response) => response.id === 31).result.content[0].text);
      expect(runtimePayload.lookupMode).toBe("heuristic");
      expect(runtimePayload.confidence).toBe("ambiguous");
      expect(runtimePayload.ambiguity).toEqual({ candidateCount: 2 });
      expect(runtimePayload.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");

      const editorResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 32,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              filePath: "samples/investigation/editor/current_panel.cpp",
            },
          },
        },
      ], dir);
      const editorPayload = JSON.parse(editorResponses.find((response) => response.id === 32).result.content[0].text);
      expect(editorPayload.lookupMode).toBe("heuristic");
      expect(editorPayload.confidence).toBe("ambiguous");
      expect(editorPayload.ambiguity).toEqual({ candidateCount: 2 });
      expect(editorPayload.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("uses anchor-qualified context to rank ambiguous UpdateShot candidates without explicit artifact hints", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const runtimeResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 33,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              anchorQualifiedName: "Game::Investigation::RunHintWorkflow",
            },
          },
        },
      ], dir);
      const runtimePayload = JSON.parse(runtimeResponses.find((response) => response.id === 33).result.content[0].text);
      expect(runtimePayload.lookupMode).toBe("heuristic");
      expect(runtimePayload.confidence).toBe("ambiguous");
      expect(runtimePayload.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");

      const editorResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 34,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              anchorQualifiedName: "Game::Editor::RefreshShotPreview",
            },
          },
        },
      ], dir);
      const editorPayload = JSON.parse(editorResponses.find((response) => response.id === 34).result.content[0].text);
      expect(editorPayload.lookupMode).toBe("heuristic");
      expect(editorPayload.confidence).toBe("ambiguous");
      expect(editorPayload.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("uses recent exact symbol context to rank ambiguous UpdateShot candidates when no explicit anchor is provided", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const runtimeResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 35,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              recentQualifiedName: "Game::Investigation::HintController::hintedPower",
            },
          },
        },
      ], dir);
      const runtimePayload = JSON.parse(runtimeResponses.find((response) => response.id === 35).result.content[0].text);
      expect(runtimePayload.lookupMode).toBe("heuristic");
      expect(runtimePayload.confidence).toBe("ambiguous");
      expect(runtimePayload.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");

      const editorResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 36,
          method: "tools/call",
          params: {
            name: "lookup_function",
            arguments: {
              name: "UpdateShot",
              recentQualifiedName: "Game::Editor::RefreshShotPreview",
            },
          },
        },
      ], dir);
      const editorPayload = JSON.parse(editorResponses.find((response) => response.id === 36).result.content[0].text);
      expect(editorPayload.lookupMode).toBe("heuristic");
      expect(editorPayload.confidence).toBe("ambiguous");
      expect(editorPayload.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("uses recent exact symbol context to resolve ambiguous find_callers targets", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const runtimeResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 37,
          method: "tools/call",
          params: {
            name: "find_callers",
            arguments: {
              name: "UpdateShot",
              limit: 10,
              recentQualifiedName: "Game::Investigation::RunHintWorkflow",
            },
          },
        },
      ], dir);
      const runtimePayload = JSON.parse(runtimeResponses.find((response) => response.id === 37).result.content[0].text);
      expect(runtimePayload.lookupMode).toBe("heuristic");
      expect(runtimePayload.confidence).toBe("ambiguous");
      expect(runtimePayload.symbol.qualifiedName).toBe("Game::Runtime::UpdateShot");
      expect(runtimePayload.callers).toHaveLength(1);
      expect(runtimePayload.callers[0].qualifiedName).toBe("Game::Runtime::TickRuntimeShot");

      const editorResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 38,
          method: "tools/call",
          params: {
            name: "find_callers",
            arguments: {
              name: "UpdateShot",
              limit: 10,
              recentQualifiedName: "Game::Editor::RefreshShotPreview",
            },
          },
        },
      ], dir);
      const editorPayload = JSON.parse(editorResponses.find((response) => response.id === 38).result.content[0].text);
      expect(editorPayload.lookupMode).toBe("heuristic");
      expect(editorPayload.confidence).toBe("ambiguous");
      expect(editorPayload.symbol.qualifiedName).toBe("Game::Editor::UpdateShot");
      expect(editorPayload.callers).toHaveLength(1);
      expect(editorPayload.callers[0].qualifiedName).toBe("Game::Editor::RefreshShotPreview");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("uses recent exact symbol context to rank ambiguous lookup_class targets", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const runtimeResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 39,
          method: "tools/call",
          params: {
            name: "lookup_class",
            arguments: {
              name: "ShotPanel",
              recentQualifiedName: "Game::Investigation::RunHintWorkflow",
            },
          },
        },
      ], dir);
      const runtimePayload = JSON.parse(runtimeResponses.find((response) => response.id === 39).result.content[0].text);
      expect(runtimePayload.lookupMode).toBe("heuristic");
      expect(runtimePayload.confidence).toBe("ambiguous");
      expect(runtimePayload.symbol.qualifiedName).toBe("Game::Runtime::ShotPanel");

      const editorResponses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 40,
          method: "tools/call",
          params: {
            name: "lookup_class",
            arguments: {
              name: "ShotPanel",
              recentQualifiedName: "Game::Editor::RefreshShotPreview",
            },
          },
        },
      ], dir);
      const editorPayload = JSON.parse(editorResponses.find((response) => response.id === 40).result.content[0].text);
      expect(editorPayload.lookupMode).toBe("heuristic");
      expect(editorPayload.confidence).toBe("ambiguous");
      expect(editorPayload.symbol.qualifiedName).toBe("Game::Editor::ShotPanel");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("exposes a compact investigate_workflow response for the QueueShot to LaunchShot scenario", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 4,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::QueueShot",
              targetQualifiedName: "Game::Investigation::LaunchShot",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 4).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::QueueShot");
      expect(payload.target.qualifiedName).toBe("Game::Investigation::LaunchShot");
      expect(payload.pathFound).toBe(true);
      expect(payload.targetConfidence).toBe("exact");
      expect(payload.pathConfidence).toBe("high");
      expect(payload.coverageConfidence).toBe("high");
      expect(payload.entry.qualifiedName).toBe("Game::Investigation::QueueShot");
      expect(payload.sink.qualifiedName).toBe("Game::Investigation::LaunchShot");
      expect(payload.mainPath).toHaveLength(2);
      expect(payload.handoffPoints.length).toBeGreaterThanOrEqual(4);
      expect(payload.mainPath[0].handoffKind).toBe("call");
      expect(payload.mainPath[1].handoffKind).toBe("call");
      expect(payload.handoffPoints.some((step: any) => step.handoffKind === "fieldWrite")).toBe(true);
      expect(payload.handoffPoints.some((step: any) => step.handoffKind === "fieldRead")).toBe(true);
      expect(Array.isArray(payload.evidence)).toBe(true);
      expect(payload.evidence.some((item: any) => item.kind === "adjacentCall")).toBe(true);
      expect(payload.evidence.some((item: any) => item.kind === "fieldAssignment")).toBe(true);
      expect(payload.evidence.some((item: any) => item.kind === "fieldRead")).toBe(true);
      expect(payload.uncertainSegments).toEqual([]);
      expect(Array.isArray(payload.diagnostics)).toBe(true);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces helper-produced hint handoffs in investigate_workflow summaries and evidence", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 41,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::RunHintWorkflow",
              targetQualifiedName: "Game::Investigation::LaunchHint",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 41).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::RunHintWorkflow");
      expect(payload.target.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.pathFound).toBe(true);
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath).toHaveLength(2);
      expect(payload.mainPath[0].from.qualifiedName).toBe("Game::Investigation::RunHintWorkflow");
      expect(payload.mainPath[0].to.qualifiedName).toBe("Game::Investigation::HintController::EmitHint");
      expect(payload.mainPath[1].to.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "inputPower"
        && step.to.expressionText === "power"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "returnValue"
        && step.from.expressionText === "hint"
        && step.to.expressionText === "hint"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "hint"
        && step.to.expressionText === "hint"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldWrite"
        && step.from.expressionText === "hint.power"
        && step.to.qualifiedName === "Game::Investigation::HintController::hintedPower"
        && step.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.from.qualifiedName === "Game::Investigation::HintController::hintedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryArgument"
        && item.relatedQualifiedName === "Game::Investigation::HintController::ApplyHint"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryReturn"
        && item.relatedQualifiedName === "Game::Investigation::MakeHint"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldAssignment"
        && item.relatedQualifiedName === "Game::Investigation::HintController::hintedPower"
        && item.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldRead"
        && item.relatedQualifiedName === "Game::Investigation::HintController::hintedPower"
      )).toBe(true);
      expect(payload.uncertainSegments).toContain("At least one workflow segment is only structurally partial rather than high-confidence.");
      expect(payload.diagnostics).toContain("At least one workflow segment crosses a callable boundary through argument-to-parameter propagation.");
      expect(payload.diagnostics).toContain("At least one workflow segment depends on a helper return value feeding the downstream workflow.");
      expect(payload.diagnostics).toContain("At least one object-state handoff is structurally weaker than a direct receiver-resolved path.");
      expect(payload.suggestedFollowUpQueries).toContain("trace_variable_flow qualifiedName=Game::Investigation::RunHintWorkflow maxDepth=4 propagationKinds=argumentToParameter,returnValue,fieldWrite,fieldRead");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_symbol qualifiedName=Game::Investigation::MakeHint");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_symbol qualifiedName=Game::Investigation::HintController::ApplyHint");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=MakeHint recentQualifiedName=Game::Investigation::RunHintWorkflow");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=ApplyHint recentQualifiedName=Game::Investigation::RunHintWorkflow");
      expect(payload.suggestedFollowUpQueries).toContain("find_callers name=MakeHint recentQualifiedName=Game::Investigation::RunHintWorkflow");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=ReadInputPower recentQualifiedName=Game::Investigation::RunHintWorkflow");
      expect(payload.suggestedFollowUpQueries).toContain("find_callers name=ReadInputPower recentQualifiedName=Game::Investigation::RunHintWorkflow");
      expect(Array.isArray(payload.suggestedLookupCandidates)).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "MakeHint"
        && candidate.symbol.qualifiedName === "Game::Investigation::MakeHint"
        && candidate.query === "lookup_function name=MakeHint recentQualifiedName=Game::Investigation::RunHintWorkflow"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunHintWorkflow"
        && candidate.contextSummary?.artifactKind === "runtime"
        && candidate.contextSummary?.subsystem === "runtime"
        && candidate.contextSummary?.module === "gameplay"
        && candidate.contextSummary?.projectArea === "investigation"
        && candidate.contextSummary?.filePath === "samples/investigation/src/workflow.cpp"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "ApplyHint"
        && candidate.symbol.qualifiedName === "Game::Investigation::HintController::ApplyHint"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunHintWorkflow"
        && candidate.contextSummary?.artifactKind === "runtime"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "ReadInputPower"
        && candidate.symbol.qualifiedName === "Game::Investigation::ReadInputPower"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunHintWorkflow"
        && candidate.contextSummary?.module === "gameplay"
      )).toBe(true);
      expect([
        payload.suggestedLookupCandidates[0].shortName,
        payload.suggestedLookupCandidates[1].shortName,
      ].sort()).toEqual(["ApplyHint", "MakeHint"]);
      expect(payload.suggestedLookupCandidates.findIndex((candidate: any) => candidate.shortName === "ReadInputPower")).toBeGreaterThan(
        payload.suggestedLookupCandidates.findIndex((candidate: any) => candidate.shortName === "MakeHint"),
      );
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces nested helper-carrier handoffs in investigate_workflow summaries and evidence", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 42,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::RunNestedHintWorkflow",
              targetQualifiedName: "Game::Investigation::LaunchHint",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 42).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::RunNestedHintWorkflow");
      expect(payload.target.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.pathFound).toBe(true);
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath).toHaveLength(2);
      expect(payload.mainPath[0].from.qualifiedName).toBe("Game::Investigation::RunNestedHintWorkflow");
      expect(payload.mainPath[0].to.qualifiedName).toBe("Game::Investigation::HintController::EmitHint");
      expect(payload.mainPath[1].to.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "inputPower"
        && step.to.expressionText === "power"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "returnValue"
        && step.from.expressionText === "nested"
        && step.to.expressionText === "nested"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "nested.envelope.hint"
        && step.to.expressionText === "hint"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldWrite"
        && step.from.expressionText === "hint.power"
        && step.to.qualifiedName === "Game::Investigation::HintController::hintedPower"
        && step.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.from.qualifiedName === "Game::Investigation::HintController::hintedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryReturn"
        && item.relatedQualifiedName === "Game::Investigation::MakeNestedEnvelope"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryArgument"
        && item.relatedQualifiedName === "Game::Investigation::HintController::ApplyHint"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldAssignment"
        && item.relatedQualifiedName === "Game::Investigation::HintController::hintedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldRead"
        && item.relatedQualifiedName === "Game::Investigation::HintController::hintedPower"
      )).toBe(true);
      expect(payload.diagnostics).toContain("At least one workflow segment crosses a callable boundary through argument-to-parameter propagation.");
      expect(payload.diagnostics).toContain("At least one workflow segment depends on a helper return value feeding the downstream workflow.");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_symbol qualifiedName=Game::Investigation::MakeNestedEnvelope");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=MakeNestedEnvelope recentQualifiedName=Game::Investigation::RunNestedHintWorkflow");
      expect(Array.isArray(payload.suggestedLookupCandidates)).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "MakeNestedEnvelope"
        && candidate.symbol.qualifiedName === "Game::Investigation::MakeNestedEnvelope"
        && candidate.query === "lookup_function name=MakeNestedEnvelope recentQualifiedName=Game::Investigation::RunNestedHintWorkflow"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunNestedHintWorkflow"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "ApplyHint"
        && candidate.symbol.qualifiedName === "Game::Investigation::HintController::ApplyHint"
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunNestedHintWorkflow"
      )).toBe(true);
      expect([
        payload.suggestedLookupCandidates[0].shortName,
        payload.suggestedLookupCandidates[1].shortName,
      ].sort()).toEqual(["ApplyHint", "MakeNestedEnvelope"]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces nested relay helper boundaries in investigate_workflow summaries and evidence", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 43,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::RunNestedRelayWorkflow",
              targetQualifiedName: "Game::Investigation::LaunchHint",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 43).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::RunNestedRelayWorkflow");
      expect(payload.target.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.pathFound).toBe(true);
      expect(payload.pathConfidence).toBe("high");
      expect(payload.coverageConfidence).toBe("high");
      expect(payload.mainPath).toHaveLength(2);
      expect(payload.mainPath[0].from.qualifiedName).toBe("Game::Investigation::RunNestedRelayWorkflow");
      expect(payload.mainPath[0].to.qualifiedName).toBe("Game::Investigation::EmitRelayHint");
      expect(payload.mainPath[1].to.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "returnValue"
        && step.from.expressionText === "nested"
        && step.to.expressionText === "nested"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "nested.envelope.hint"
        && step.to.expressionText === "hint"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "returnValue"
        && step.from.expressionText === "power"
        && step.to.expressionText === "power"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "power"
        && step.to.expressionText === "power"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryReturn"
        && item.relatedQualifiedName === "Game::Investigation::MakeNestedEnvelope"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "boundaryReturn"
        && item.relatedQualifiedName === "Game::Investigation::ExtractHintPower"
      )).toBe(true);
      expect(payload.diagnostics).toContain("At least one workflow segment crosses a callable boundary through argument-to-parameter propagation.");
      expect(payload.diagnostics).toContain("At least one workflow segment depends on a helper return value feeding the downstream workflow.");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=ExtractHintPower recentQualifiedName=Game::Investigation::RunNestedRelayWorkflow");
      expect(Array.isArray(payload.suggestedLookupCandidates)).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "ExtractHintPower"
        && candidate.symbol.qualifiedName === "Game::Investigation::ExtractHintPower"
        && candidate.query === "lookup_function name=ExtractHintPower recentQualifiedName=Game::Investigation::RunNestedRelayWorkflow"
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunNestedRelayWorkflow"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "EmitRelayHint"
        && candidate.symbol.qualifiedName === "Game::Investigation::EmitRelayHint"
        && candidate.query === "lookup_function name=EmitRelayHint recentQualifiedName=Game::Investigation::RunNestedRelayWorkflow"
        && candidate.contextSummary?.qualifiedName === "Game::Investigation::RunNestedRelayWorkflow"
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "adjacentCall"
          && item.relatedQualifiedName === "Game::Investigation::EmitRelayHint")
      )).toBe(true);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces one-more-step forwarding helper chains in investigate_workflow summaries", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 44,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::RunNestedRelayToForwarderWorkflow",
              targetQualifiedName: "Game::Investigation::LaunchHint",
              maxDepth: 5,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 44).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::RunNestedRelayToForwarderWorkflow");
      expect(payload.target.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.pathFound).toBe(true);
      expect(payload.pathConfidence).toBe("high");
      expect(payload.coverageConfidence).toBe("high");
      expect(payload.mainPath).toHaveLength(3);
      expect(payload.mainPath[0].to.qualifiedName).toBe("Game::Investigation::EmitForwardedPower");
      expect(payload.mainPath[1].to.qualifiedName).toBe("Game::Investigation::EmitRelayHint");
      expect(payload.mainPath[2].to.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "nested.envelope.hint"
        && step.to.qualifiedName === "Game::Investigation::ExtractHintPower"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "argumentToParameter"
        && step.from.expressionText === "power"
        && step.to.qualifiedName === "Game::Investigation::EmitForwardedPower"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "EmitForwardedPower"
        && candidate.symbol.qualifiedName === "Game::Investigation::EmitForwardedPower"
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "adjacentCall"
          && item.relatedQualifiedName === "Game::Investigation::EmitForwardedPower")
      )).toBe(true);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("exposes partial workflow diagnostics for pointer-heavy member-state investigation", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 5,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::PendingState::armed",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 5).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::PendingState::armed");
      expect(payload.pathFound).toBe(true);
      expect(payload.target).toBeUndefined();
      expect(payload.targetConfidence).toBe("exact");
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath.length).toBeGreaterThanOrEqual(1);
      expect(payload.handoffPoints.length).toBeGreaterThanOrEqual(2);
      expect(payload.mainPath.some((step: any) => step.handoffKind === "fieldRead")).toBe(true);
      expect(payload.handoffPoints.some((step: any) => step.handoffKind === "fieldWrite")).toBe(true);
      expect(payload.handoffPoints.some((step: any) => step.risks.includes("pointerHeavyFlow"))).toBe(true);
      expect(Array.isArray(payload.evidence)).toBe(true);
      expect(payload.evidence.some((item: any) => item.risks.includes("pointerHeavyFlow"))).toBe(true);
      expect(payload.uncertainSegments).toContain("At least one workflow segment is only structurally partial rather than high-confidence.");
      expect(payload.uncertainSegments).toContain("Pointer-heavy flow interrupts full-confidence continuity in at least one segment.");
      expect(payload.diagnostics).toContain("Pointer-heavy flow appears in the stitched workflow, so alias-sensitive continuity may be incomplete.");
      expect(payload.diagnostics).toContain("The returned workflow should be treated as partial guidance rather than complete proof.");
      expect(payload.suggestedFollowUpQueries).toContain("trace_variable_flow qualifiedName=Game::Investigation::PendingState::armed maxDepth=3");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces constructor-seeded field handoffs in investigate_workflow summaries and evidence", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 7,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::ConstructedHintController::seededPower",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 7).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::ConstructedHintController::seededPower");
      expect(payload.pathFound).toBe(true);
      expect(payload.target).toBeUndefined();
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.from.qualifiedName === "Game::Investigation::ConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldWrite"
        && step.ownerQualifiedName === "Game::Investigation::ConstructedHintController::ConstructedHintController"
        && step.to.qualifiedName === "Game::Investigation::ConstructedHintController::seededPower"
        && step.from.expressionText === "MakeHint(initialPower).power"
        && step.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.ownerQualifiedName === "Game::Investigation::ConstructedHintController::EmitConstructed"
        && step.from.qualifiedName === "Game::Investigation::ConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldAssignment"
        && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::seededPower"
        && item.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldRead"
        && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "ownerContext"
        && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::ConstructedHintController"
        && item.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "ownerContext"
        && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::EmitConstructed"
      )).toBe(true);
      expect(Array.isArray(payload.suggestedLookupCandidates)).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "EmitConstructed"
        && candidate.symbol.qualifiedName === "Game::Investigation::ConstructedHintController::EmitConstructed"
        && candidate.query === "lookup_function name=EmitConstructed recentQualifiedName=Game::Investigation::ConstructedHintController::seededPower"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "ownerContext"
          && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::EmitConstructed")
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "ConstructedHintController"
        && candidate.symbol.qualifiedName === "Game::Investigation::ConstructedHintController::ConstructedHintController"
        && candidate.advisory === "Suggested owning callable under partial workflow coverage; inspect the callable before treating it as a definitive continuation."
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "ownerContext"
          && item.relatedQualifiedName === "Game::Investigation::ConstructedHintController::ConstructedHintController")
      )).toBe(true);
      expect([
        payload.suggestedLookupCandidates[0].shortName,
        payload.suggestedLookupCandidates[1].shortName,
      ].sort()).toEqual(["ConstructedHintController", "EmitConstructed"]);
      expect(payload.uncertainSegments).toContain("At least one workflow segment is only structurally partial rather than high-confidence.");
      expect(payload.diagnostics).toContain("At least one object-state handoff is structurally weaker than a direct receiver-resolved path.");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("keeps field-centric relay workflows alive through helper boundaries", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 17,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::RelayFieldController::storedPower",
              targetQualifiedName: "Game::Investigation::LaunchHint",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 17).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::RelayFieldController::storedPower");
      expect(payload.target?.qualifiedName).toBe("Game::Investigation::LaunchHint");
      expect(payload.pathFound).toBe(true);
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.from.qualifiedName === "Game::Investigation::RelayFieldController::storedPower"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldWrite"
        && step.ownerQualifiedName === "Game::Investigation::RelayFieldController::ApplyHint"
        && step.to.qualifiedName === "Game::Investigation::RelayFieldController::storedPower"
        && step.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.ownerQualifiedName === "Game::Investigation::RelayFieldController::EmitStored"
        && step.from.qualifiedName === "Game::Investigation::RelayFieldController::storedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldAssignment"
        && item.relatedQualifiedName === "Game::Investigation::RelayFieldController::storedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldRead"
        && item.relatedQualifiedName === "Game::Investigation::RelayFieldController::storedPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "ownerContext"
        && item.relatedQualifiedName === "Game::Investigation::RelayFieldController::EmitStored"
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "EmitStored"
        && candidate.symbol.qualifiedName === "Game::Investigation::RelayFieldController::EmitStored"
      )).toBe(true);
      expect(payload.uncertainSegments).toContain("At least one workflow segment is only structurally partial rather than high-confidence.");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("surfaces mixed nested-boundary and constructor-seeded handoffs in one investigation", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 8,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::NestedConstructedHintController::seededPower",
              maxDepth: 4,
              maxEdges: 20,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 8).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::NestedConstructedHintController::seededPower");
      expect(payload.pathFound).toBe(true);
      expect(payload.target).toBeUndefined();
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("partial");
      expect(payload.mainPath.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.from.qualifiedName === "Game::Investigation::NestedConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldWrite"
        && step.ownerQualifiedName === "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController"
        && step.to.qualifiedName === "Game::Investigation::NestedConstructedHintController::seededPower"
        && step.from.expressionText === "MakeNestedEnvelope(initialPower).envelope.hint.power"
        && step.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.handoffPoints.some((step: any) =>
        step.handoffKind === "fieldRead"
        && step.ownerQualifiedName === "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed"
        && step.from.qualifiedName === "Game::Investigation::NestedConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldAssignment"
        && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::seededPower"
        && item.risks.includes("receiverAmbiguity")
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "fieldRead"
        && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::seededPower"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "ownerContext"
        && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController"
      )).toBe(true);
      expect(payload.evidence.some((item: any) =>
        item.kind === "ownerContext"
        && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed"
      )).toBe(true);
      expect(Array.isArray(payload.suggestedLookupCandidates)).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "EmitNestedConstructed"
        && candidate.symbol.qualifiedName === "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed"
        && candidate.query === "lookup_function name=EmitNestedConstructed recentQualifiedName=Game::Investigation::NestedConstructedHintController::seededPower"
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "ownerContext"
          && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::EmitNestedConstructed")
      )).toBe(true);
      expect(payload.suggestedLookupCandidates.some((candidate: any) =>
        candidate.shortName === "NestedConstructedHintController"
        && candidate.symbol.qualifiedName === "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController"
        && candidate.supportingEvidence?.some((item: any) =>
          item.kind === "ownerContext"
          && item.relatedQualifiedName === "Game::Investigation::NestedConstructedHintController::NestedConstructedHintController")
      )).toBe(true);
      expect(payload.diagnostics).toContain("At least one object-state handoff is structurally weaker than a direct receiver-resolved path.");
      expect(payload.uncertainSegments).toContain("At least one workflow segment is only structurally partial rather than high-confidence.");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("marks zero-result investigation as weak coverage when the source sits in a parse-fragile region", async () => {
    const { dir } = writeFixtureJsonStore();
    try {
      const responses = await mcpCall([
        INIT,
        INITIALIZED,
        {
          jsonrpc: "2.0",
          id: 6,
          method: "tools/call",
          params: {
            name: "investigate_workflow",
            arguments: {
              sourceQualifiedName: "Game::Investigation::WeakProbe",
              maxDepth: 3,
              maxEdges: 10,
            },
          },
        },
      ], dir);
      const payload = JSON.parse(responses.find((response) => response.id === 6).result.content[0].text);
      expect(payload.source.qualifiedName).toBe("Game::Investigation::WeakProbe");
      expect(payload.pathFound).toBe(false);
      expect(payload.mainPath).toEqual([]);
      expect(payload.handoffPoints).toEqual([]);
      expect(payload.evidence).toHaveLength(1);
      expect(payload.evidence[0].kind).toBe("adjacentCall");
      expect(payload.evidence[0].relatedQualifiedName).toBe("Game::Investigation::WeakHelper");
      expect(payload.pathConfidence).toBe("partial");
      expect(payload.coverageConfidence).toBe("weak");
      expect(payload.uncertainSegments).toContain("No bounded workflow continuation was found from the requested exact source.");
      expect(payload.diagnostics).toContain("No bounded workflow path was found from the requested exact source.");
      expect(payload.diagnostics).toContain("Nearby file risk signals suggest the absence of a stronger path may reflect weak coverage rather than true structural absence.");
      expect(payload.diagnostics).toContain("This symbol lives in a parse-fragile file, so structurally exact results may still sit near unstable syntax.");
      expect(payload.suggestedFollowUpQueries).toContain("list_file_symbols filePath=samples/investigation/src/partial_flow.cpp");
      expect(payload.suggestedFollowUpQueries).toContain("lookup_function name=WeakHelper recentQualifiedName=Game::Investigation::WeakProbe");
      expect(payload.suggestedLookupCandidates).toHaveLength(1);
      expect(payload.suggestedLookupCandidates[0].shortName).toBe("WeakHelper");
      expect(payload.suggestedLookupCandidates[0].symbol.qualifiedName).toBe("Game::Investigation::WeakHelper");
      expect(payload.suggestedLookupCandidates[0].advisory).toBe("Suggested owning callable under weak workflow coverage; inspect the callable and nearby file context before treating it as definitive.");
      expect(payload.suggestedLookupCandidates[0].contextSummary?.qualifiedName).toBe("Game::Investigation::WeakProbe");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
