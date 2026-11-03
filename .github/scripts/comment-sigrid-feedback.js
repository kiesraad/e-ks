const fs = require('node:fs');

module.exports = async function commentSigridFeedback({ github, context }) {
  const marker = '<!-- sigrid-feedback -->';
  const feedbackPath = 'sigrid-ci-output/feedback.md';

  if (!fs.existsSync(feedbackPath)) {
    console.log(`No Sigrid feedback file found at ${feedbackPath}; skipping PR comment.`);
    return;
  }

  const feedback = fs.readFileSync(feedbackPath, 'utf8').trim();

  if (!feedback) {
    console.log('Sigrid feedback file is empty; skipping PR comment.');
    return;
  }

  const body = `${marker}\n${feedback}`;

  const comments = await github.paginate(github.rest.issues.listComments, {
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
