# GitLab CI

vsg-rs writes GitLab's code-quality report (`--quality_report`) and JUnit (`-j`), so violations
show up in the merge request widget and in the pipeline's test tab.

```yaml
vhdl-style:
  stage: test
  image: python:3.13-slim
  script:
    - pip install --no-cache-dir vsg-rs==0.11.0
    - >
      vsg-rs -c vsg.yaml --recursive src
      --quality_report gl-code-quality-report.json
      -j junit.xml
      --statistics
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
      junit: junit.xml
```

* **Merge request widget**: the code-quality report lists the violations the branch adds,
  with file and line. It needs the artifact declared under `reports.codequality`.
* **Test tab**: the JUnit file turns each file into a test case, which makes the history of a
  rule visible over pipelines.
* **Job log**: `--statistics` prints the violations per rule, so a failing job says what to fix
  first.
* The job fails when vsg-rs exits with 1 (error-severity violations). Add
  `allow_failure: true` while adopting the style, and drop it once the code is clean.

Fixing locally uses the same configuration:

```sh
pip install vsg-rs
vsg-rs -c vsg.yaml --recursive src --fix
```
