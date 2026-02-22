이터레이션 $ARGUMENTS 실행:

1. `docs/iterations/iter-$ARGUMENTS.md` 파일을 읽고 작업 내용을 파악한다
2. 작업 내용을 구현한다
3. `cargo build`로 빌드 확인
4. `cargo test`로 테스트 통과 확인
5. 변경사항을 커밋한다 (메시지: `feat($ARGUMENTS): 작업 요약`)
6. push는 하지 않는다
7. 다음 이터레이션이 있으면 알려준다
