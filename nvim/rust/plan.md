  # Smear Cursor 최고 설계안 v2: Window 경계를 포함한 전역 Jump Bridge 프레젠
  테이션

  ## Implementation Checklist

  - [x] Reducer bookkeeping: introduce `MotionClass` and a bounded `JumpCue` chain in runtime
    state so discontinuous jumps become explicit state-machine transitions.
  - [x] Reducer presentation semantics: emit immediate draw-capable discontinuous-jump cue frames
    instead of only relying on legacy clear-or-retarget behavior.
  - [x] Render-plan projection: map jump cues into global display-space bridge geometry across
    window separators.
- [x] Render-plan robustness: preserve bridge continuity across sparse undecodable cells while
  keeping destination catch salience dominant.
  - [ ] Golden/performance coverage: lock alternating cross-window jump scenarios behind bounded
    cue-chain tests and planner-cost assertions.

  ## Summary

  플러그인은 커서의 실제 상태와 화면 표현을 분리한다.

  - Truth lane은 canonical cursor, target, observation validity, epoch,
    lifecycle만 관리한다.
  - Presentation lane은 continuous smear와 authored jump bridge를 합성한다.

  핵심 정책은 다음과 같이 고정한다.

  - 작은 이동은 Continuous
  - 큰 점프는 DiscontinuousJump
  - window가 달라도 큰 점프는 동일한 DiscontinuousJump로 취급
  - cross-window jump도 tail/bridge animation을 반드시 보여준다
  - Settling은 sustained trail 제어에만 관여하고, 첫 jump acknowledgment를 막
    지 않는다
  - 빠른 반복 점프는 매번 독립 cue를 생성하되 bounded chain으로 관리한다

  ## Key Changes

  - motion classification을 다음 세 종류로 단순화한다:
      - Continuous
      - DiscontinuousJump
      - SurfaceRetarget
  - window change는 더 이상 기본적으로 visual cut이 아니다.
      - same-window든 cross-window든 display-space 상 큰 leap면
        DiscontinuousJump
      - 단, truth상 surface tracking은 계속 정확히 갱신
  - presentation은 전체 화면을 하나의 global display plane으로 보고 jump cue를
    생성한다.
      - split border, statuscolumn, win separator를 포함한 화면 좌표계를 가로
        지르는 bridge 허용
  - DiscontinuousJump의 visual grammar:
      - 출발점 launch bloom
      - 전체 거리 방향을 드러내는 full bridge filament
      - 도착점 catch bloom
      - 필요 시 짧은 fade veil
  - old trail은 독립 drain
  - new jump는 즉시 cue 생성
  - cross-window jump도 bridge의 세기나 존재를 약화하지 않는다
  - source selection은 “same surface인가”보다 “같은 visual grammar로 처리 가능
    한가”를 본다.
      - 큰 jump는 always fresh presentation event
      - ordinary local motion만 in-flight retarget tick으로 흡수 가능

  ## Presentation Model

  ### Global display space

  - 모든 jump cue geometry는 active window local 좌표가 아니라 화면 기준
    global display metric에서 계산한다.
  - 각 window의 cursor pose는 먼저 global display pose로 변환한 뒤 cue
    geometry를 만든다.
  - bridge는 global plane 위에서 직선 또는 약한 곡률의 directed stroke로 생성
    한다.

  ### Jump cue phases

  JumpCue는 다음 phase를 가진다.

  - Launch
  - Transfer
  - Catch
  - Fade

  기본 동작:

  - Launch: 출발 위치에 압축된 밝은 burst
  - Transfer: 출발과 도착을 잇는 강한 filament/bridge
  - Catch: 도착 지점에서 더 강한 수렴형 시각 효과
  - Fade: 최신 cue를 가리지 않게 짧게 사라짐

  ### Cross-window bridge policy

  - Full bridge를 기본 정책으로 채택
  - border나 빈 공간을 만났다고 cue를 끊지 않음
  - bridge는 “실제 intermediate cursor”가 아니라 “jump acknowledgment”로서
    global plane을 관통
  - separator 위에서도 decode 가능한 셀에 한해 그릴 수 있으면 그린다
  - 그릴 수 없는 셀은 sparse하게 건너뛰되, 지각상 bridge continuity는 유지한다

  ### Bounded chaining

  - active jump cue 최대 개수는 3
  - newest cue가 salience 최상위
  - oldest cue부터 빠르게 fade 또는 evict
  - burst 길이와 무관하게 planning/apply 비용이 bounded해야 한다

  ## Truth / Reducer Semantics

  ### Truth lane responsibilities

  - canonical cursor pose
  - tracked (window, buffer)
  - target pose
  - pending settle
  - lifecycle phase
  - retarget epoch
  - stale timer/observation suppression

  ### Discontinuous jump reducer behavior

  - target은 즉시 새 위치로 갱신
  - tracked surface도 즉시 새 값으로 갱신
  - presentation에 JumpCue 생성 이벤트를 emit
  - 첫 acknowledgment frame은 무조건 draw 가능해야 함
  - settle 결과와 무관하게 첫 cue는 항상 보임
  - old continuous stroke는 drain queue로 이동

  ### SurfaceRetarget

  - 관측/epoch/target 갱신은 발생하지만 jump threshold 미만이거나 visual cue가
    불필요한 경우
  - 일반 retarget bookkeeping용
  - 구현자는 이를 visual cut semantics로 사용하지 않는다

  ## Interfaces / Types

  추가 타입:

  - MotionClass
  - JumpCue
  - PresentationScene
  - PresentationEpoch
  - GlobalDisplayPose
  - NavigationIntent

  핵심 필드:

  - JumpCue { cue_id, epoch, from_pose, to_pose, started_at_ms, duration_ms,
    strength, phase }
  - GlobalDisplayPose { row_display, col_display, window_handle,
    buffer_handle }

  추가 config:

  - jump_cues_enabled: bool = true
  - jump_cue_min_display_distance
  - jump_cue_duration_ms: f64 = 84.0
  - jump_cue_strength
  - jump_cue_max_chain: u8 = 3
  - jump_intent_window_ms: f64 = 40.0
  - cross_window_jump_bridges: bool = true
  - cross_window_bridge_strength_scale: f64 = 1.0

  기본값은 cross-window에서도 same-window와 동등한 강도로 보이게 설정한다.

  ## Scenario Behavior

  ### 1. 같은 window 안의 작은 이동

  - Continuous
  - 기존 smear/comet 사용
  - jump cue 없음

  ### 2. 같은 window 안의 큰 jump

  - DiscontinuousJump
  - full bridge cue 즉시 생성
  - old trail drain
  - settle은 후속 continuous behavior만 제어

  ### 3. 다른 window로 한 번 jump

  - DiscontinuousJump
  - 출발 window에서 launch
  - separator를 가로지르는 global bridge
  - 도착 window에서 강한 catch
  - 사용자에게 하나의 큰 spatial event로 읽혀야 함

  ### 4. 서로 다른 두 window 사이를 빠르게 왕복

  - 매 점프마다 새 cue 생성
  - bridge는 매번 border를 가로질러 보임
  - cue chain은 bounded
  - 최신 왕복의 리듬감이 살아야 함

  ### 5. gg, G와 window jump가 섞인 경우

  - 모두 큰 leap로 분류되면 동일한 jump grammar 사용
  - 점프 종류가 달라도 visual language는 통일
  - 사용자는 “긴 거리 이동은 항상 시원하게 보인다”는 규칙성을 체감해야 함

  ## Test Plan

  ### Reducer tests

  - cross-window large move classifies as DiscontinuousJump
  - cross-window discontinuous jump emits immediate draw-capable cue
  - same-window and cross-window large jumps share cue creation semantics
  - stale timer cannot overwrite newer jump cue epoch
  - repeated cross-window jumps cap cue chain length

  ### Render-plan tests

  - bridge geometry spans global display space across separator
  - sparse undecodable cells near separator do not visually sever bridge
    continuity
  - destination catch dominates long bridge tail
  - old draining trail and new cross-window bridge coexist stably
  - jump cue remains visible for at least one committed frame per jump

  ### Golden scenarios

  - left split -> right split single jump
  - left <-> right rapid alternating jumps
  - top split <-> bottom split rapid alternating jumps
  - same-window gg/G mixed with cross-window jump
  - long-distance multi-split jump under bounded cue chain

  ### Performance tests

  - repeated cross-window jumps do not cause unbounded active scene growth
  - planning cost remains bounded by jump_cue_max_chain
  - diff/apply remains stable under rapid bridge churn

  ## Assumptions / Defaults

  - 사용자는 window 경계를 시각적 discontinuity가 아니라 spatial jump의 일부로
    느끼길 원한다.
  - 따라서 cross-window jump는 끊지 않고 bridge tail을 보여주는 것이 기본값이
    다.
  - bridge는 truth path가 아니라 presentation cue다.
  - separator/빈 공간을 가로지르는 표현은 허용한다.
  - cleanliness보다 jump acknowledgment를 우선한다.
  - same-window large jump와 cross-window large jump는 같은 visual language를
    사용한다.
