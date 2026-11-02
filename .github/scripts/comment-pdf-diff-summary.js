const fs = require('node:fs');

module.exports = async function commentPdfDiffSummary({ github, context }) {
  const marker = '<!-- pdf-diff-summary -->';
  const results = fs.readFileSync('tmp/results.md', 'utf8').trim();
  const artifactUrl = process.env.ARTIFACT_URL;
  const baseBody = [
    marker,
    '## PDF diff summary',
    '',
    results,
  ].join('\n');
  const body = artifactUrl
    ? `${baseBody}\n\n[Download diff artifacts](${artifactUrl})`
    : baseBody;

  const { data: comments } = await github.rest.issues.listComments({
    owner: context.repo.owner,
    repo: context.repo.repo,
    issue_number: context.issue.number,
  });

  const existingComment = comments.find(
    (comment) => comment.user?.type === 'Bot' && comment.body?.includes(marker)
  );

  if (existingComment) {
    await github.rest.issues.updateComment({
      owner: context.repo.owner,
      repo: context.repo.repo,
      comment_id: existingComment.id,
      body,
    });
    return;
  }

  await github.rest.issues.createComment({
    owner: context.repo.owner,
    repo: context.repo.repo,
    issue_number: context.issue.number,
    body,
  });
};
